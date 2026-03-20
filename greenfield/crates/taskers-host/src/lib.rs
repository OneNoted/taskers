use anyhow::{Result, anyhow, bail};
use gtk::{
    Align, Box as GtkBox, CssProvider, EventControllerFocus, EventControllerScroll,
    EventControllerScrollFlags, Fixed, GestureClick, Orientation, Overflow, Overlay,
    STYLE_PROVIDER_PRIORITY_APPLICATION, Widget, glib, prelude::*,
};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use taskers_core::{
    BrowserMountSpec, HostCommand, HostEvent, PortalSurfacePlan, ShellDragMode, ShellSnapshot,
    SurfaceId, SurfaceMountSpec, SurfacePortalPlan, TerminalMountSpec,
};
use taskers_domain::PaneKind;
use taskers_ghostty::{GhosttyHost, SurfaceDescriptor};
use webkit6::{Settings as WebKitSettings, WebView, prelude::*};

pub type HostEventSink = Rc<dyn Fn(HostEvent) + 'static>;
pub type DiagnosticsSink = Arc<dyn Fn(DiagnosticRecord) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCategory {
    Startup,
    Window,
    Sync,
    HostEvent,
    SurfaceLifecycle,
    BrowserMetadata,
    Smoke,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub timestamp_ms: u128,
    pub revision: Option<u64>,
    pub category: DiagnosticCategory,
    pub message: String,
    pub pane_id: Option<taskers_core::PaneId>,
    pub surface_id: Option<SurfaceId>,
}

impl DiagnosticRecord {
    pub fn new(
        category: DiagnosticCategory,
        revision: Option<u64>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            timestamp_ms: current_timestamp_ms(),
            revision,
            category,
            message: message.into(),
            pane_id: None,
            surface_id: None,
        }
    }

    pub fn with_pane(mut self, pane_id: taskers_core::PaneId) -> Self {
        self.pane_id = Some(pane_id);
        self
    }

    pub fn with_surface(mut self, surface_id: SurfaceId) -> Self {
        self.surface_id = Some(surface_id);
        self
    }

    pub fn format_line(&self) -> String {
        let revision = self
            .revision
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        let pane = self
            .pane_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());
        let surface = self
            .surface_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into());

        format!(
            "ts_ms={} category={:?} revision={} pane={} surface={} message={}",
            self.timestamp_ms, self.category, revision, pane, surface, self.message
        )
    }
}

pub struct TaskersHost {
    root: Overlay,
    surface_layer: Fixed,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
    ghostty_host: Option<GhosttyHost>,
    browser_surfaces: HashMap<SurfaceId, BrowserSurface>,
    terminal_surfaces: HashMap<SurfaceId, TerminalSurface>,
}

impl TaskersHost {
    pub fn new(
        shell_widget: &impl IsA<Widget>,
        ghostty_host: Option<GhosttyHost>,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Self {
        let root = Overlay::new();
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_child(Some(shell_widget));
        install_native_surface_css();

        let surface_layer = Fixed::new();
        surface_layer.set_hexpand(true);
        surface_layer.set_vexpand(true);
        // The surface layer spans the full window, but only mounted native pane
        // bodies should intercept pointer events. Leaving the layer targetable
        // blocks the shared shell webview underneath.
        surface_layer.set_can_target(false);
        root.add_overlay(&surface_layer);

        let pan_sink = event_sink.clone();
        let pan_diagnostics = diagnostics.clone();
        let workspace_pan = EventControllerScroll::new(EventControllerScrollFlags::BOTH_AXES);
        workspace_pan.set_propagation_phase(gtk::PropagationPhase::Capture);
        workspace_pan.connect_scroll(move |_, dx, dy| {
            let Some((dx, dy)) = workspace_pan_delta(dx, dy) else {
                return glib::Propagation::Proceed;
            };
            emit_diagnostic(
                pan_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("workspace pan gesture dx={dx} dy={dy}"),
                ),
            );
            (pan_sink)(HostEvent::ViewportScrolled { dx, dy });
            glib::Propagation::Proceed
        });
        root.add_controller(workspace_pan);

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Window,
                None,
                "created GTK4 host overlay",
            ),
        );

        Self {
            root,
            surface_layer,
            event_sink,
            diagnostics,
            ghostty_host,
            browser_surfaces: HashMap::new(),
            terminal_surfaces: HashMap::new(),
        }
    }

    pub fn widget(&self) -> Overlay {
        self.root.clone()
    }

    pub fn sync_snapshot(&mut self, snapshot: &ShellSnapshot) -> Result<()> {
        let interactive = native_surfaces_interactive(snapshot.drag_mode);
        emit_diagnostic(
            self.diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::Sync,
                Some(snapshot.revision),
                format!("host sync start panes={}", snapshot.portal.panes.len()),
            ),
        );
        self.sync_browser_surfaces(&snapshot.portal, snapshot.revision, interactive)?;
        self.sync_terminal_surfaces(&snapshot.portal, snapshot.revision, interactive)?;
        Ok(())
    }

    pub fn tick(&self) {
        if let Some(host) = &self.ghostty_host {
            let _ = host.tick();
        }
    }

    pub fn handle_command(&mut self, command: HostCommand) -> Result<()> {
        match command {
            HostCommand::BrowserNavigate { surface_id, url } => {
                self.with_browser_surface(surface_id, "browser navigate", |surface| {
                    surface.navigate(&url)
                })
            }
            HostCommand::BrowserBack { surface_id } => {
                self.with_browser_surface(surface_id, "browser back", |surface| surface.go_back())
            }
            HostCommand::BrowserForward { surface_id } => {
                self.with_browser_surface(surface_id, "browser forward", |surface| {
                    surface.go_forward()
                })
            }
            HostCommand::BrowserReload { surface_id } => {
                self.with_browser_surface(surface_id, "browser reload", |surface| surface.reload())
            }
            HostCommand::BrowserToggleDevtools { surface_id } => {
                self.with_browser_surface(surface_id, "browser devtools toggle", |surface| {
                    surface.toggle_devtools()
                })
            }
        }
    }

    fn with_browser_surface(
        &mut self,
        surface_id: SurfaceId,
        action: &'static str,
        callback: impl FnOnce(&mut BrowserSurface),
    ) -> Result<()> {
        let Some(surface) = self.browser_surfaces.get_mut(&surface_id) else {
            return Ok(());
        };
        callback(surface);
        emit_diagnostic(
            self.diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::HostEvent,
                None,
                format!("{action} command handled"),
            )
            .with_surface(surface_id),
        );
        Ok(())
    }

    fn sync_browser_surfaces(
        &mut self,
        portal: &SurfacePortalPlan,
        revision: u64,
        interactive: bool,
    ) -> Result<()> {
        let desired = browser_plans(portal);
        let desired_ids = desired
            .iter()
            .map(|plan| plan.surface_id)
            .collect::<HashSet<_>>();

        let stale = self
            .browser_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();

        for surface_id in stale {
            if let Some(surface) = self.browser_surfaces.remove(&surface_id) {
                surface.shell.detach(&self.surface_layer);
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::SurfaceLifecycle,
                        Some(revision),
                        "browser surface removed",
                    )
                    .with_surface(surface_id),
                );
            }
        }

        for plan in desired {
            match self.browser_surfaces.get_mut(&plan.surface_id) {
                Some(surface) => surface.sync(
                    &self.surface_layer,
                    &plan,
                    revision,
                    interactive,
                    self.diagnostics.as_ref(),
                )?,
                None => {
                    let surface = BrowserSurface::new(
                        &self.surface_layer,
                        &plan,
                        revision,
                        interactive,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                    )?;
                    self.browser_surfaces.insert(plan.surface_id, surface);
                }
            }
        }

        Ok(())
    }

    fn sync_terminal_surfaces(
        &mut self,
        portal: &SurfacePortalPlan,
        revision: u64,
        interactive: bool,
    ) -> Result<()> {
        let desired = terminal_plans(portal);
        let desired_ids = desired
            .iter()
            .map(|plan| plan.surface_id)
            .collect::<HashSet<_>>();

        let stale = self
            .terminal_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();

        for surface_id in stale {
            if let Some(surface) = self.terminal_surfaces.remove(&surface_id) {
                surface.shell.detach(&self.surface_layer);
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::SurfaceLifecycle,
                        Some(revision),
                        "terminal surface removed",
                    )
                    .with_surface(surface_id),
                );
            }
        }

        let Some(host) = self.ghostty_host.as_ref() else {
            return Ok(());
        };

        for plan in desired {
            match self.terminal_surfaces.get_mut(&plan.surface_id) {
                Some(surface) => surface.sync(
                    &self.surface_layer,
                    plan.frame,
                    plan.active,
                    revision,
                    interactive,
                    host,
                    self.diagnostics.as_ref(),
                ),
                None => {
                    let surface = TerminalSurface::new(
                        &self.surface_layer,
                        &plan,
                        revision,
                        interactive,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                        host,
                    )?;
                    self.terminal_surfaces.insert(plan.surface_id, surface);
                }
            }
        }

        Ok(())
    }
}

struct BrowserSurface {
    shell: NativeSurfaceShell,
    surface_id: SurfaceId,
    webview: WebView,
    url: String,
    active: bool,
    interactive: bool,
    devtools_open: Rc<Cell<bool>>,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
}

impl BrowserSurface {
    fn new(
        fixed: &Fixed,
        plan: &PortalSurfacePlan,
        revision: u64,
        interactive: bool,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Result<Self> {
        let BrowserMountSpec { url } = browser_spec(plan)?.clone();

        let settings = WebKitSettings::builder()
            .enable_back_forward_navigation_gestures(true)
            .enable_developer_extras(true)
            .build();
        let webview = WebView::builder()
            .hexpand(true)
            .vexpand(true)
            .focusable(true)
            .settings(&settings)
            .build();
        let (shell_class, widget_class) = native_surface_classes(PaneKind::Browser);
        webview.add_css_class("native-surface-widget");
        webview.add_css_class(widget_class);
        webview.set_can_target(interactive);
        webview.load_uri(&url);
        (event_sink)(HostEvent::SurfaceUrlChanged {
            surface_id: plan.surface_id,
            url: url.clone(),
        });
        let shell = NativeSurfaceShell::new(shell_class);
        shell.mount_child(webview.upcast_ref());
        shell.position(fixed, plan.frame);
        let devtools_open = Rc::new(Cell::new(false));

        let pane_id = plan.pane_id;
        let surface_id = plan.surface_id;
        let focus_sink = event_sink.clone();
        let focus_diagnostics = diagnostics.clone();
        let focus = EventControllerFocus::new();
        focus.connect_enter(move |_| {
            emit_diagnostic(
                focus_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    "browser focus event received",
                )
                .with_pane(pane_id)
                .with_surface(surface_id),
            );
            (focus_sink)(HostEvent::PaneFocused { pane_id });
        });
        webview.add_controller(focus);

        let surface_id = plan.surface_id;
        let title_sink = event_sink.clone();
        let title_diagnostics = diagnostics.clone();
        webview.connect_title_notify(move |web_view| {
            if let Some(title) = web_view.title() {
                emit_diagnostic(
                    title_diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::BrowserMetadata,
                        None,
                        format!("browser title observed: {title}"),
                    )
                    .with_surface(surface_id),
                );
                (title_sink)(HostEvent::SurfaceTitleChanged {
                    surface_id,
                    title: title.to_string(),
                });
            }
        });

        let surface_id = plan.surface_id;
        let navigation_sink = event_sink.clone();
        let navigation_diagnostics = diagnostics.clone();
        let navigation_devtools = devtools_open.clone();
        webview.connect_load_changed(move |web_view, _| {
            emit_browser_navigation_state(
                web_view,
                surface_id,
                navigation_devtools.get(),
                &navigation_sink,
                navigation_diagnostics.as_ref(),
            );
        });

        let url_surface_id = plan.surface_id;
        let url_sink = event_sink;
        let uri_sink = url_sink.clone();
        let url_diagnostics = diagnostics.clone();
        let navigation_sink = url_sink.clone();
        let navigation_diagnostics = diagnostics.clone();
        let navigation_devtools = devtools_open.clone();
        webview.connect_uri_notify(move |web_view| {
            if let Some(url) = web_view.uri() {
                emit_diagnostic(
                    url_diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::BrowserMetadata,
                        None,
                        format!("browser url observed: {url}"),
                    )
                    .with_surface(url_surface_id),
                );
                (uri_sink)(HostEvent::SurfaceUrlChanged {
                    surface_id: url_surface_id,
                    url: url.to_string(),
                });
            }
            emit_browser_navigation_state(
                web_view,
                url_surface_id,
                navigation_devtools.get(),
                &navigation_sink,
                navigation_diagnostics.as_ref(),
            );
        });

        if let Some(inspector) = webview.inspector() {
            let navigation_webview = webview.clone();
            let navigation_sink = url_sink.clone();
            let navigation_diagnostics = diagnostics.clone();
            let navigation_devtools = devtools_open.clone();
            let inspector_surface_id = plan.surface_id;
            inspector.connect_closed(move |_| {
                navigation_devtools.set(false);
                emit_browser_navigation_state(
                    &navigation_webview,
                    inspector_surface_id,
                    false,
                    &navigation_sink,
                    navigation_diagnostics.as_ref(),
                );
            });
        }

        if plan.active && interactive {
            webview.grab_focus();
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface created",
            )
            .with_pane(plan.pane_id)
            .with_surface(plan.surface_id),
        );

        emit_browser_navigation_state(
            &webview,
            plan.surface_id,
            devtools_open.get(),
            &url_sink,
            diagnostics.as_ref(),
        );

        Ok(Self {
            shell,
            surface_id: plan.surface_id,
            webview,
            url,
            active: plan.active,
            interactive,
            devtools_open,
            event_sink: url_sink,
            diagnostics,
        })
    }

    fn sync(
        &mut self,
        fixed: &Fixed,
        plan: &PortalSurfacePlan,
        revision: u64,
        interactive: bool,
        diagnostics: Option<&DiagnosticsSink>,
    ) -> Result<()> {
        self.shell.position(fixed, plan.frame);
        self.webview.set_can_target(interactive);

        let BrowserMountSpec { url } = browser_spec(plan)?;
        if self.url != *url {
            self.webview.load_uri(url);
            self.url = url.clone();
        }
        if plan.active && interactive && (!self.active || !self.interactive) {
            self.webview.grab_focus();
        }
        self.active = plan.active;
        self.interactive = interactive;

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface updated",
            )
            .with_pane(plan.pane_id)
            .with_surface(plan.surface_id),
        );

        Ok(())
    }

    fn navigate(&mut self, url: &str) {
        if self.url != url {
            self.webview.load_uri(url);
            self.url = url.to_string();
        }
        self.webview.grab_focus();
        self.emit_navigation_state();
    }

    fn go_back(&mut self) {
        if self.webview.can_go_back() {
            self.webview.go_back();
        }
        self.webview.grab_focus();
        self.emit_navigation_state();
    }

    fn go_forward(&mut self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward();
        }
        self.webview.grab_focus();
        self.emit_navigation_state();
    }

    fn reload(&mut self) {
        self.webview.reload();
        self.webview.grab_focus();
        self.emit_navigation_state();
    }

    fn toggle_devtools(&mut self) {
        let Some(inspector) = self.webview.inspector() else {
            return;
        };
        if self.devtools_open.get() {
            inspector.close();
        } else {
            if inspector.can_attach() {
                inspector.attach();
            }
            inspector.show();
            self.devtools_open.set(true);
        }
        self.webview.grab_focus();
        self.emit_navigation_state();
    }

    fn emit_navigation_state(&self) {
        emit_browser_navigation_state(
            &self.webview,
            self.surface_id,
            self.devtools_open.get(),
            &self.event_sink,
            self.diagnostics.as_ref(),
        );
    }
}

struct TerminalSurface {
    shell: NativeSurfaceShell,
    widget: Widget,
    active: bool,
    interactive: bool,
}

impl TerminalSurface {
    fn new(
        fixed: &Fixed,
        plan: &PortalSurfacePlan,
        revision: u64,
        interactive: bool,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
        host: &GhosttyHost,
    ) -> Result<Self> {
        let spec = terminal_spec(plan)?.clone();
        let descriptor = surface_descriptor_from(&spec);
        let widget = host
            .create_surface(&descriptor)
            .map_err(|error| anyhow!(error.to_string()))?;
        widget.set_hexpand(true);
        widget.set_vexpand(true);
        widget.set_halign(Align::Fill);
        widget.set_valign(Align::Fill);
        widget.set_focusable(true);
        let (shell_class, widget_class) = native_surface_classes(PaneKind::Terminal);
        widget.add_css_class("native-surface-widget");
        widget.add_css_class(widget_class);
        widget.add_css_class("terminal-output");
        widget.set_can_target(interactive);
        let shell = NativeSurfaceShell::new(shell_class);
        shell.mount_child(&widget);
        shell.position(fixed, plan.frame);

        connect_ghostty_widget(host, &widget, plan, event_sink, diagnostics.clone());

        if plan.active && interactive {
            let _ = host.focus_surface(&widget);
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface created",
            )
            .with_pane(plan.pane_id)
            .with_surface(plan.surface_id),
        );

        Ok(Self {
            shell,
            widget,
            active: plan.active,
            interactive,
        })
    }

    fn sync(
        &mut self,
        fixed: &Fixed,
        frame: taskers_core::Frame,
        active: bool,
        revision: u64,
        interactive: bool,
        host: &GhosttyHost,
        diagnostics: Option<&DiagnosticsSink>,
    ) {
        self.widget.set_can_target(interactive);
        self.shell.position(fixed, frame);
        if active && interactive && (!self.active || !self.interactive) {
            let _ = host.focus_surface(&self.widget);
        }
        self.active = active;
        self.interactive = interactive;

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface updated",
            ),
        );
    }
}

struct NativeSurfaceShell {
    root: GtkBox,
}

impl NativeSurfaceShell {
    fn new(kind_class: &'static str) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.set_hexpand(true);
        root.set_vexpand(true);
        root.set_halign(Align::Fill);
        root.set_valign(Align::Fill);
        root.set_overflow(Overflow::Hidden);
        root.set_focusable(false);
        root.set_can_target(false);
        root.add_css_class("native-surface-host");
        root.add_css_class(kind_class);
        Self { root }
    }

    fn mount_child(&self, child: &Widget) {
        child.set_hexpand(true);
        child.set_vexpand(true);
        child.set_halign(Align::Fill);
        child.set_valign(Align::Fill);
        if child.parent().is_none() {
            self.root.append(child);
        }
    }

    fn position(&self, fixed: &Fixed, frame: taskers_core::Frame) {
        position_widget(fixed, self.root.upcast_ref(), frame);
    }

    fn detach(&self, fixed: &Fixed) {
        detach_from_fixed(fixed, self.root.upcast_ref());
    }
}

fn install_native_surface_css() {
    let provider = CssProvider::new();
    provider.load_from_data(native_surface_css());
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

fn native_surface_classes(kind: PaneKind) -> (&'static str, &'static str) {
    match kind {
        PaneKind::Terminal => ("native-surface-terminal", "native-surface-terminal-widget"),
        PaneKind::Browser => ("native-surface-browser", "native-surface-browser-widget"),
    }
}

fn native_surface_css() -> &'static str {
    r#"
.native-surface-host,
.native-surface-widget,
.terminal-output {
  margin: 0;
  padding: 0;
  border-radius: 0;
  box-shadow: none;
}

.native-surface-terminal,
.native-surface-terminal-widget,
.terminal-output {
  background: #0f1117;
}

.native-surface-browser,
.native-surface-browser-widget {
  background: transparent;
}
"#
}

fn connect_ghostty_widget(
    host: &GhosttyHost,
    widget: &Widget,
    plan: &PortalSurfacePlan,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
) {
    let _ = host;

    let pane_id = plan.pane_id;
    let surface_id = plan.surface_id;
    let focus_sink = event_sink.clone();
    let focus_diagnostics = diagnostics.clone();
    let click = GestureClick::new();
    click.connect_pressed(move |_, _, _, _| {
        emit_diagnostic(
            focus_diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::HostEvent,
                None,
                "terminal click focus event received",
            )
            .with_pane(pane_id)
            .with_surface(surface_id),
        );
        (focus_sink)(HostEvent::PaneFocused { pane_id });
    });
    widget.add_controller(click);

    let pane_id = plan.pane_id;
    let surface_id = plan.surface_id;
    let focus_sink = event_sink.clone();
    let focus_diagnostics = diagnostics.clone();
    let focus = EventControllerFocus::new();
    focus.connect_enter(move |_| {
        emit_diagnostic(
            focus_diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::HostEvent,
                None,
                "terminal focus event received",
            )
            .with_pane(pane_id)
            .with_surface(surface_id),
        );
        (focus_sink)(HostEvent::PaneFocused { pane_id });
    });
    widget.add_controller(focus);

    let surface_id = plan.surface_id;
    let title_sink = event_sink.clone();
    let title_diagnostics = diagnostics.clone();
    widget.connect_notify_local(Some("title"), move |widget, _| {
        if let Some(title) = widget.property::<Option<glib::GString>>("title") {
            emit_diagnostic(
                title_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("terminal title observed: {title}"),
                )
                .with_surface(surface_id),
            );
            (title_sink)(HostEvent::SurfaceTitleChanged {
                surface_id,
                title: title.to_string(),
            });
        }
    });

    let surface_id = plan.surface_id;
    let cwd_sink = event_sink.clone();
    let cwd_diagnostics = diagnostics.clone();
    widget.connect_notify_local(Some("pwd"), move |widget, _| {
        if let Some(cwd) = widget.property::<Option<glib::GString>>("pwd") {
            emit_diagnostic(
                cwd_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    format!("terminal cwd observed: {cwd}"),
                )
                .with_surface(surface_id),
            );
            (cwd_sink)(HostEvent::SurfaceCwdChanged {
                surface_id,
                cwd: cwd.to_string(),
            });
        }
    });

    let pane_id = plan.pane_id;
    let surface_id = plan.surface_id;
    let exit_sink = event_sink;
    let exit_diagnostics = diagnostics;
    widget.connect_notify_local(Some("child-exited"), move |widget, _| {
        if widget.property::<bool>("child-exited") {
            emit_diagnostic(
                exit_diagnostics.as_ref(),
                DiagnosticRecord::new(DiagnosticCategory::HostEvent, None, "terminal child exited")
                    .with_pane(pane_id)
                    .with_surface(surface_id),
            );
            (exit_sink)(HostEvent::SurfaceClosed {
                pane_id,
                surface_id,
            });
        }
    });
}

fn emit_browser_navigation_state(
    webview: &WebView,
    surface_id: SurfaceId,
    devtools_open: bool,
    event_sink: &HostEventSink,
    diagnostics: Option<&DiagnosticsSink>,
) {
    emit_diagnostic(
        diagnostics,
        DiagnosticRecord::new(
            DiagnosticCategory::BrowserMetadata,
            None,
            format!(
                "browser navigation state updated back={} forward={} devtools={}",
                webview.can_go_back(),
                webview.can_go_forward(),
                devtools_open
            ),
        )
        .with_surface(surface_id),
    );
    (event_sink)(HostEvent::BrowserNavigationStateChanged {
        surface_id,
        can_go_back: webview.can_go_back(),
        can_go_forward: webview.can_go_forward(),
        devtools_open,
    });
}

fn surface_descriptor_from(spec: &TerminalMountSpec) -> SurfaceDescriptor {
    SurfaceDescriptor {
        cols: spec.cols,
        rows: spec.rows,
        kind: PaneKind::Terminal,
        cwd: spec.cwd.clone(),
        title: None,
        url: None,
        // The current Ghostty bridge is more stable when it controls shell
        // selection itself, so keep command overrides empty until that path is
        // proven across hosts.
        command_argv: Vec::new(),
        env: spec.env.clone(),
    }
}

fn browser_spec(plan: &PortalSurfacePlan) -> Result<&BrowserMountSpec> {
    match &plan.mount {
        SurfaceMountSpec::Browser(spec) => Ok(spec),
        SurfaceMountSpec::Terminal(_) => bail!("surface {} is not a browser", plan.surface_id),
    }
}

fn terminal_spec(plan: &PortalSurfacePlan) -> Result<&TerminalMountSpec> {
    match &plan.mount {
        SurfaceMountSpec::Terminal(spec) => Ok(spec),
        SurfaceMountSpec::Browser(_) => bail!("surface {} is not a terminal", plan.surface_id),
    }
}

fn position_widget(fixed: &Fixed, widget: &Widget, frame: taskers_core::Frame) {
    widget.set_size_request(frame.width.max(1), frame.height.max(1));
    if widget.parent().is_some() {
        fixed.move_(widget, f64::from(frame.x), f64::from(frame.y));
    } else {
        fixed.put(widget, f64::from(frame.x), f64::from(frame.y));
    }
}

fn detach_from_fixed(fixed: &Fixed, widget: &Widget) {
    if widget.parent().is_some() {
        fixed.remove(widget);
    }
}

fn native_surfaces_interactive(drag_mode: ShellDragMode) -> bool {
    drag_mode == ShellDragMode::None
}

fn workspace_pan_delta(dx: f64, dy: f64) -> Option<(i32, i32)> {
    if !dx.is_finite() || !dy.is_finite() {
        return None;
    }
    if dx.abs() < 1.0 || dx.abs() < dy.abs() {
        return None;
    }
    Some((dx.round() as i32, 0))
}

pub fn browser_plans(portal: &SurfacePortalPlan) -> Vec<PortalSurfacePlan> {
    portal
        .panes
        .iter()
        .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Browser(_)))
        .filter_map(|plan| clip_to_content(plan, &portal.content))
        .collect()
}

pub fn terminal_plans(portal: &SurfacePortalPlan) -> Vec<PortalSurfacePlan> {
    portal
        .panes
        .iter()
        .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Terminal(_)))
        .filter_map(|plan| clip_to_content(plan, &portal.content))
        .collect()
}

fn clip_to_content(
    plan: &PortalSurfacePlan,
    content: &taskers_core::Frame,
) -> Option<PortalSurfacePlan> {
    let f = &plan.frame;
    let cx = content.x;
    let cy = content.y;
    let cr = content.x + content.width;
    let cb = content.y + content.height;

    let clipped_x = f.x.max(cx);
    let clipped_y = f.y.max(cy);
    let clipped_r = (f.x + f.width).min(cr);
    let clipped_b = (f.y + f.height).min(cb);

    let clipped_w = clipped_r - clipped_x;
    let clipped_h = clipped_b - clipped_y;

    if clipped_w <= 0 || clipped_h <= 0 {
        return None;
    }

    Some(PortalSurfacePlan {
        frame: taskers_core::Frame::new(clipped_x, clipped_y, clipped_w, clipped_h),
        ..plan.clone()
    })
}

fn emit_diagnostic(sink: Option<&DiagnosticsSink>, record: DiagnosticRecord) {
    if let Some(sink) = sink {
        sink(record);
    }
}

fn current_timestamp_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        browser_plans, native_surface_classes, native_surface_css, native_surfaces_interactive,
        terminal_plans, workspace_pan_delta,
    };
    use taskers_core::{BootstrapModel, SharedCore, ShellDragMode, SurfaceMountSpec};
    use taskers_domain::PaneKind;

    #[test]
    fn partitions_portal_plans_by_surface_kind() {
        let core = SharedCore::bootstrap(BootstrapModel::default());
        let snapshot = core.snapshot();

        let browsers = browser_plans(&snapshot.portal);
        let terminals = terminal_plans(&snapshot.portal);

        assert_eq!(browsers.len(), 1);
        assert_eq!(terminals.len(), 1);
        assert!(matches!(browsers[0].mount, SurfaceMountSpec::Browser(_)));
        assert!(matches!(terminals[0].mount, SurfaceMountSpec::Terminal(_)));
    }

    #[test]
    fn workspace_pan_delta_prefers_deliberate_horizontal_motion() {
        assert_eq!(workspace_pan_delta(64.4, 4.0), Some((64, 0)));
        assert_eq!(workspace_pan_delta(0.4, 0.0), None);
        assert_eq!(workspace_pan_delta(6.0, 18.0), None);
        assert_eq!(workspace_pan_delta(f64::NAN, 0.0), None);
    }

    #[test]
    fn native_surfaces_disable_pointer_targeting_during_shell_drags() {
        assert!(native_surfaces_interactive(ShellDragMode::None));
        assert!(!native_surfaces_interactive(ShellDragMode::Window));
        assert!(!native_surfaces_interactive(ShellDragMode::Surface));
    }

    #[test]
    fn native_surface_classes_distinguish_terminal_and_browser_hosts() {
        assert_eq!(
            native_surface_classes(PaneKind::Terminal),
            ("native-surface-terminal", "native-surface-terminal-widget")
        );
        assert_eq!(
            native_surface_classes(PaneKind::Browser),
            ("native-surface-browser", "native-surface-browser-widget")
        );
    }

    #[test]
    fn native_surface_css_restores_terminal_background_contract() {
        let css = native_surface_css();
        assert!(css.contains(".native-surface-terminal"));
        assert!(css.contains(".native-surface-terminal-widget"));
        assert!(css.contains(".terminal-output"));
        assert!(css.contains("background: #0f1117;"));
    }
}
