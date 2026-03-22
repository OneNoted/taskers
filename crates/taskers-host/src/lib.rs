mod browser_automation;

use anyhow::{Result, anyhow};
use gtk::{
    Align, Box as GtkBox, CssProvider, EventControllerFocus, EventControllerScroll,
    EventControllerScrollFlags, GestureClick, Orientation, Overflow, Overlay,
    STYLE_PROVIDER_PRIORITY_APPLICATION, Widget, glib, prelude::*,
};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use taskers_control::{
    BrowserControlCommand, BrowserLoadState, ControlError, TerminalDebugCommand,
    TerminalDebugResult, TerminalRenderStats,
};
use taskers_core::{
    BrowserSurfaceCatalogEntry, HostCommand, HostEvent, PaneId, PortalSurfacePlan, ShellDragMode,
    ShellSnapshot, SurfaceId, SurfaceMountSpec, SurfacePortalPlan, TerminalMountSpec,
    TerminalSurfaceCatalogEntry, WorkspaceId,
};
use taskers_domain::PaneKind;
use taskers_ghostty::{GhosttyHost, SurfaceDescriptor};
use taskers_shell_core as taskers_core;
use webkit6::{LoadEvent, Settings as WebKitSettings, WebView, prelude::*};

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
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
    ghostty_host: Option<GhosttyHost>,
    browser_surfaces: HashMap<SurfaceId, BrowserSurface>,
    terminal_surfaces: HashMap<SurfaceId, TerminalSurface>,
}

#[derive(Clone)]
pub struct BrowserSurfaceHandle {
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    webview: WebView,
    last_load_state: Rc<Cell<Option<BrowserLoadState>>>,
}

impl BrowserSurfaceHandle {
    pub fn surface_id(&self) -> SurfaceId {
        self.surface_id
    }

    pub fn workspace_id(&self) -> WorkspaceId {
        self.workspace_id.get()
    }

    pub fn pane_id(&self) -> PaneId {
        self.pane_id.get()
    }

    pub(crate) fn webview(&self) -> &WebView {
        &self.webview
    }

    pub(crate) fn url(&self) -> String {
        self.webview
            .uri()
            .map(|uri| uri.to_string())
            .unwrap_or_default()
    }

    pub(crate) fn title(&self) -> String {
        self.webview
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default()
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.webview.is_loading()
    }

    pub(crate) fn load_state(&self) -> Option<BrowserLoadState> {
        self.last_load_state.get()
    }

    pub(crate) fn focus_webview(&self) {
        self.webview.grab_focus();
    }

    pub(crate) fn is_webview_focused(&self) -> bool {
        self.webview.has_focus()
    }

    pub(crate) fn navigate(&self, url: &str) {
        self.webview.load_uri(url);
    }

    pub(crate) fn go_back(&self) {
        if self.webview.can_go_back() {
            self.webview.go_back();
        }
    }

    pub(crate) fn go_forward(&self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward();
        }
    }

    pub(crate) fn reload(&self) {
        self.webview.reload();
    }
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
        shell_widget.add_controller(workspace_pan);

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
        self.sync_browser_surfaces(snapshot, interactive)?;
        self.sync_terminal_surfaces(
            &snapshot.portal,
            &snapshot.terminal_catalog,
            snapshot.revision,
            interactive,
        )?;
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

    pub async fn execute_browser_command(
        &self,
        command: BrowserControlCommand,
    ) -> Result<serde_json::Value, ControlError> {
        let surface_id = browser_command_surface_id(&command);
        let handle = self.browser_surface_handle(surface_id)?;
        handle.execute(command).await
    }

    pub fn execute_terminal_debug(
        &self,
        command: TerminalDebugCommand,
    ) -> Result<TerminalDebugResult, ControlError> {
        let Some(host) = self.ghostty_host.as_ref() else {
            return Err(ControlError::not_supported(
                "terminal debug requires the Ghostty host backend",
            ));
        };

        let surface_id = terminal_debug_surface_id(&command);
        let surface = self.terminal_surfaces.get(&surface_id).ok_or_else(|| {
            ControlError::not_found(format!("terminal surface {surface_id} not found"))
        })?;

        match command {
            TerminalDebugCommand::IsFocused { .. } => Ok(TerminalDebugResult::IsFocused {
                focused: surface.is_focused(),
            }),
            TerminalDebugCommand::ReadText { tail_lines, .. } => {
                let text = host
                    .read_surface_text(&surface.widget)
                    .map_err(|error| ControlError::internal(error.to_string()))?;
                Ok(TerminalDebugResult::ReadText {
                    text: trim_terminal_tail(text, tail_lines),
                })
            }
            TerminalDebugCommand::RenderStats { .. } => {
                let has_selection = host
                    .surface_has_selection(&surface.widget)
                    .map_err(|error| ControlError::internal(error.to_string()))?;
                Ok(TerminalDebugResult::RenderStats {
                    stats: TerminalRenderStats {
                        surface_id,
                        workspace_id: surface.workspace_id.get(),
                        pane_id: surface.pane_id.get(),
                        mounted: true,
                        visible: surface.visible,
                        focused: surface.is_focused(),
                        backend: "ghostty".into(),
                        cols: surface.spec.cols,
                        rows: surface.spec.rows,
                        width_px: surface.width_px,
                        height_px: surface.height_px,
                        has_selection,
                    },
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

    pub fn browser_surface_handle(
        &self,
        surface_id: SurfaceId,
    ) -> Result<BrowserSurfaceHandle, ControlError> {
        self.browser_surfaces
            .get(&surface_id)
            .map(BrowserSurface::handle)
            .ok_or_else(|| {
                ControlError::not_found(format!("browser surface {surface_id} not found"))
            })
    }

    fn sync_browser_surfaces(&mut self, snapshot: &ShellSnapshot, interactive: bool) -> Result<()> {
        let desired = browser_plans(&snapshot.portal);
        let desired_by_id = desired
            .into_iter()
            .map(|plan| (plan.surface_id, plan))
            .collect::<HashMap<_, _>>();
        let catalog_by_id = snapshot
            .browser_catalog
            .iter()
            .map(|entry| (entry.surface_id, entry))
            .collect::<HashMap<_, _>>();
        let desired_ids = catalog_by_id.keys().copied().collect::<HashSet<_>>();

        let stale = self
            .browser_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();

        for surface_id in stale {
            if let Some(surface) = self.browser_surfaces.remove(&surface_id) {
                surface.shell.detach(&self.root);
                emit_diagnostic(
                    self.diagnostics.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::SurfaceLifecycle,
                        Some(snapshot.revision),
                        "browser surface removed",
                    )
                    .with_surface(surface_id),
                );
            }
        }

        for entry in snapshot.browser_catalog.iter() {
            let visible_plan = desired_by_id.get(&entry.surface_id);
            match self.browser_surfaces.get_mut(&entry.surface_id) {
                Some(surface) => surface.sync(
                    &self.root,
                    entry,
                    visible_plan,
                    snapshot.revision,
                    interactive,
                    self.diagnostics.as_ref(),
                )?,
                None => {
                    let surface = BrowserSurface::new(
                        &self.root,
                        entry,
                        visible_plan,
                        snapshot.revision,
                        interactive,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                    )?;
                    self.browser_surfaces.insert(entry.surface_id, surface);
                }
            }
        }

        Ok(())
    }

    fn sync_terminal_surfaces(
        &mut self,
        portal: &SurfacePortalPlan,
        catalog: &[TerminalSurfaceCatalogEntry],
        revision: u64,
        interactive: bool,
    ) -> Result<()> {
        let desired = terminal_plans(portal);
        let desired_by_id = desired
            .into_iter()
            .map(|plan| (plan.surface_id, plan))
            .collect::<HashMap<_, _>>();
        let catalog_by_id = catalog
            .iter()
            .map(|entry| (entry.surface_id, entry))
            .collect::<HashMap<_, _>>();
        let desired_ids = catalog_by_id.keys().copied().collect::<HashSet<_>>();

        let stale = self
            .terminal_surfaces
            .keys()
            .copied()
            .filter(|surface_id| !desired_ids.contains(surface_id))
            .collect::<Vec<_>>();

        for surface_id in stale {
            if let Some(surface) = self.terminal_surfaces.remove(&surface_id) {
                surface.shell.detach(&self.root);
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

        for entry in catalog {
            let visible_plan = desired_by_id.get(&entry.surface_id);
            match self.terminal_surfaces.get_mut(&entry.surface_id) {
                Some(surface) => surface.sync(
                    &self.root,
                    entry,
                    visible_plan,
                    revision,
                    interactive,
                    host,
                    self.diagnostics.as_ref(),
                ),
                None => {
                    let surface = TerminalSurface::new(
                        &self.root,
                        entry,
                        visible_plan,
                        revision,
                        interactive,
                        self.event_sink.clone(),
                        self.diagnostics.clone(),
                        host,
                    )?;
                    self.terminal_surfaces.insert(entry.surface_id, surface);
                }
            }
        }

        Ok(())
    }
}

struct BrowserSurface {
    shell: NativeSurfaceShell,
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    webview: WebView,
    url: String,
    active: bool,
    interactive: bool,
    visible: bool,
    devtools_open: Rc<Cell<bool>>,
    last_load_state: Rc<Cell<Option<BrowserLoadState>>>,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
}

impl BrowserSurface {
    fn new(
        overlay: &Overlay,
        entry: &BrowserSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        revision: u64,
        interactive: bool,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
    ) -> Result<Self> {
        let url = entry.url.clone();

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
        webview.set_can_target(visible_plan.is_some() && interactive);
        webview.load_uri(&url);
        (event_sink)(HostEvent::SurfaceUrlChanged {
            surface_id: entry.surface_id,
            url: url.clone(),
        });
        let shell = NativeSurfaceShell::new(shell_class, visible_plan.is_some() && interactive);
        shell.mount_child(webview.upcast_ref());
        match visible_plan {
            Some(plan) => shell.show_at(overlay, plan.frame),
            None => shell.park_hidden(overlay),
        }
        let devtools_open = Rc::new(Cell::new(false));
        let workspace_id = Rc::new(Cell::new(entry.workspace_id));
        let pane_id = Rc::new(Cell::new(entry.pane_id));
        let last_load_state = Rc::new(Cell::new(None));

        let focus_pane_id = pane_id.clone();
        let surface_id = entry.surface_id;
        let focus_sink = event_sink.clone();
        let focus_diagnostics = diagnostics.clone();
        let focus = EventControllerFocus::new();
        focus.connect_enter(move |_| {
            let pane_id = focus_pane_id.get();
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

        let click_pane_id = pane_id.clone();
        let click_sink = event_sink.clone();
        let click_diagnostics = diagnostics.clone();
        let click = GestureClick::new();
        click.connect_pressed(move |_, _, _, _| {
            let pane_id = click_pane_id.get();
            emit_diagnostic(
                click_diagnostics.as_ref(),
                DiagnosticRecord::new(
                    DiagnosticCategory::HostEvent,
                    None,
                    "browser click focus event received",
                )
                .with_pane(pane_id)
                .with_surface(surface_id),
            );
            (click_sink)(HostEvent::PaneFocused { pane_id });
        });
        webview.add_controller(click);

        let surface_id = entry.surface_id;
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

        let surface_id = entry.surface_id;
        let navigation_sink = event_sink.clone();
        let navigation_diagnostics = diagnostics.clone();
        let navigation_devtools = devtools_open.clone();
        let load_state_cell = last_load_state.clone();
        webview.connect_load_changed(move |web_view, load_event| {
            load_state_cell.set(Some(browser_load_state_from_webkit(load_event)));
            emit_browser_navigation_state(
                web_view,
                surface_id,
                navigation_devtools.get(),
                &navigation_sink,
                navigation_diagnostics.as_ref(),
            );
        });

        let url_surface_id = entry.surface_id;
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
            let inspector_surface_id = entry.surface_id;
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

        if visible_plan.is_some_and(|plan| plan.active) && interactive {
            webview.grab_focus();
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface created",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        emit_browser_navigation_state(
            &webview,
            entry.surface_id,
            devtools_open.get(),
            &url_sink,
            diagnostics.as_ref(),
        );

        Ok(Self {
            shell,
            surface_id: entry.surface_id,
            workspace_id,
            pane_id,
            webview,
            url,
            active: visible_plan.is_some_and(|plan| plan.active),
            interactive: visible_plan.is_some() && interactive,
            visible: visible_plan.is_some(),
            devtools_open,
            last_load_state,
            event_sink: url_sink,
            diagnostics,
        })
    }

    fn sync(
        &mut self,
        overlay: &Overlay,
        entry: &BrowserSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        revision: u64,
        interactive: bool,
        diagnostics: Option<&DiagnosticsSink>,
    ) -> Result<()> {
        self.workspace_id.set(entry.workspace_id);
        self.pane_id.set(entry.pane_id);
        let visible = visible_plan.is_some();
        let effective_interactive = visible && interactive;
        self.shell.set_interactive(effective_interactive);
        self.webview.set_can_target(effective_interactive);
        match visible_plan {
            Some(plan) => self.shell.show_at(overlay, plan.frame),
            None => self.shell.park_hidden(overlay),
        }

        if self.url != entry.url {
            self.webview.load_uri(&entry.url);
            self.url = entry.url.clone();
        }
        if visible_plan.is_some_and(|plan| plan.active)
            && effective_interactive
            && (!self.active || !self.interactive || !self.visible)
        {
            self.webview.grab_focus();
        }
        self.active = visible_plan.is_some_and(|plan| plan.active);
        self.interactive = effective_interactive;
        self.visible = visible;

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "browser surface updated",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        Ok(())
    }

    fn navigate(&mut self, url: &str) {
        if self.url != url {
            self.webview.load_uri(url);
            self.url = url.to_string();
        }
        self.emit_navigation_state();
    }

    fn go_back(&mut self) {
        if self.webview.can_go_back() {
            self.webview.go_back();
        }
        self.emit_navigation_state();
    }

    fn go_forward(&mut self) {
        if self.webview.can_go_forward() {
            self.webview.go_forward();
        }
        self.emit_navigation_state();
    }

    fn reload(&mut self) {
        self.webview.reload();
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

    fn handle(&self) -> BrowserSurfaceHandle {
        BrowserSurfaceHandle {
            surface_id: self.surface_id,
            workspace_id: self.workspace_id.clone(),
            pane_id: self.pane_id.clone(),
            webview: self.webview.clone(),
            last_load_state: self.last_load_state.clone(),
        }
    }
}

struct TerminalSurface {
    surface_id: SurfaceId,
    workspace_id: Rc<Cell<WorkspaceId>>,
    pane_id: Rc<Cell<PaneId>>,
    spec: TerminalMountSpec,
    shell: NativeSurfaceShell,
    widget: Widget,
    focus_state: Rc<Cell<bool>>,
    active: bool,
    interactive: bool,
    visible: bool,
    width_px: i32,
    height_px: i32,
}

impl TerminalSurface {
    fn new(
        overlay: &Overlay,
        entry: &TerminalSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        revision: u64,
        interactive: bool,
        event_sink: HostEventSink,
        diagnostics: Option<DiagnosticsSink>,
        host: &GhosttyHost,
    ) -> Result<Self> {
        let spec = entry.spec.clone();
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
        let effective_interactive = visible_plan.is_some() && interactive;
        widget.set_can_target(effective_interactive);
        let shell = NativeSurfaceShell::new(shell_class, effective_interactive);
        shell.mount_child(&widget);
        match visible_plan {
            Some(plan) => shell.show_at(overlay, plan.frame),
            None => shell.park_hidden(overlay),
        }

        let workspace_id = Rc::new(Cell::new(entry.workspace_id));
        let pane_id = Rc::new(Cell::new(entry.pane_id));
        let focus_state = Rc::new(Cell::new(false));
        connect_ghostty_widget(
            host,
            &widget,
            pane_id.clone(),
            entry.surface_id,
            event_sink,
            diagnostics.clone(),
            focus_state.clone(),
        );

        if visible_plan.is_some_and(|plan| plan.active) && effective_interactive {
            let _ = host.focus_surface(&widget);
        }

        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface created",
            )
            .with_pane(entry.pane_id)
            .with_surface(entry.surface_id),
        );

        Ok(Self {
            surface_id: entry.surface_id,
            workspace_id,
            pane_id,
            spec,
            shell,
            widget,
            focus_state,
            active: visible_plan.is_some_and(|plan| plan.active),
            interactive: effective_interactive,
            visible: visible_plan.is_some(),
            width_px: visible_plan.map_or(0, |plan| plan.frame.width),
            height_px: visible_plan.map_or(0, |plan| plan.frame.height),
        })
    }

    fn sync(
        &mut self,
        overlay: &Overlay,
        entry: &TerminalSurfaceCatalogEntry,
        visible_plan: Option<&PortalSurfacePlan>,
        revision: u64,
        interactive: bool,
        host: &GhosttyHost,
        diagnostics: Option<&DiagnosticsSink>,
    ) {
        self.workspace_id.set(entry.workspace_id);
        self.pane_id.set(entry.pane_id);
        self.spec = entry.spec.clone();
        let visible = visible_plan.is_some();
        let effective_interactive = visible && interactive;
        self.widget.set_can_target(effective_interactive);
        self.shell.set_interactive(effective_interactive);
        match visible_plan {
            Some(plan) => self.shell.show_at(overlay, plan.frame),
            None => self.shell.park_hidden(overlay),
        }
        if visible_plan.is_some_and(|plan| plan.active)
            && effective_interactive
            && (!self.active || !self.interactive || !self.visible)
        {
            let _ = host.focus_surface(&self.widget);
        }
        if !visible || !effective_interactive {
            self.focus_state.set(false);
        }
        self.active = visible_plan.is_some_and(|plan| plan.active);
        self.interactive = effective_interactive;
        self.visible = visible;
        self.width_px = visible_plan.map_or(0, |plan| plan.frame.width);
        self.height_px = visible_plan.map_or(0, |plan| plan.frame.height);

        emit_diagnostic(
            diagnostics,
            DiagnosticRecord::new(
                DiagnosticCategory::SurfaceLifecycle,
                Some(revision),
                "terminal surface updated",
            )
            .with_pane(entry.pane_id)
            .with_surface(self.surface_id),
        );
    }

    fn is_focused(&self) -> bool {
        self.focus_state.get() || self.widget.has_focus()
    }
}

struct NativeSurfaceShell {
    root: GtkBox,
}

impl NativeSurfaceShell {
    fn new(kind_class: &'static str, interactive: bool) -> Self {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.set_hexpand(false);
        root.set_vexpand(false);
        root.set_halign(Align::Start);
        root.set_valign(Align::Start);
        root.set_overflow(Overflow::Hidden);
        root.set_focusable(false);
        root.set_can_target(interactive);
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

    fn position(&self, overlay: &Overlay, frame: taskers_core::Frame) {
        position_widget(overlay, self.root.upcast_ref(), frame);
    }

    fn show_at(&self, overlay: &Overlay, frame: taskers_core::Frame) {
        self.root.set_opacity(1.0);
        self.position(overlay, frame);
    }

    fn park_hidden(&self, overlay: &Overlay) {
        self.root.set_opacity(0.0);
        self.position(overlay, self.hidden_frame());
    }

    fn set_interactive(&self, interactive: bool) {
        self.root.set_can_target(interactive);
    }

    fn detach(&self, overlay: &Overlay) {
        detach_from_overlay(overlay, self.root.upcast_ref());
    }

    fn hidden_frame(&self) -> taskers_core::Frame {
        taskers_core::Frame::new(100_000, 100_000, 1, 1)
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
    pane_id: Rc<Cell<PaneId>>,
    surface_id: SurfaceId,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
    focus_state: Rc<Cell<bool>>,
) {
    let _ = host;

    let click_pane_id = pane_id.clone();
    let focus_sink = event_sink.clone();
    let focus_diagnostics = diagnostics.clone();
    let click = GestureClick::new();
    click.connect_pressed(move |_, _, _, _| {
        let pane_id = click_pane_id.get();
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

    let focus_pane_id = pane_id.clone();
    let focus_sink = event_sink.clone();
    let focus_diagnostics = diagnostics.clone();
    let focus_enter_state = focus_state.clone();
    let focus = EventControllerFocus::new();
    focus.connect_enter(move |_| {
        let pane_id = focus_pane_id.get();
        focus_enter_state.set(true);
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
    let focus_leave_state = focus_state;
    focus.connect_leave(move |_| {
        focus_leave_state.set(false);
    });
    widget.add_controller(focus);

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

    let exit_pane_id = pane_id;
    let exit_sink = event_sink;
    let exit_diagnostics = diagnostics;
    widget.connect_notify_local(Some("child-exited"), move |widget, _| {
        if widget.property::<bool>("child-exited") {
            let pane_id = exit_pane_id.get();
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

fn browser_load_state_from_webkit(load_event: LoadEvent) -> BrowserLoadState {
    match load_event {
        LoadEvent::Started => BrowserLoadState::Started,
        LoadEvent::Redirected => BrowserLoadState::Redirected,
        LoadEvent::Committed => BrowserLoadState::Committed,
        LoadEvent::Finished => BrowserLoadState::Finished,
        _ => BrowserLoadState::Finished,
    }
}

fn browser_command_surface_id(command: &BrowserControlCommand) -> SurfaceId {
    match command {
        BrowserControlCommand::Navigate { surface_id, .. }
        | BrowserControlCommand::Back { surface_id }
        | BrowserControlCommand::Forward { surface_id }
        | BrowserControlCommand::Reload { surface_id }
        | BrowserControlCommand::FocusWebview { surface_id }
        | BrowserControlCommand::IsWebviewFocused { surface_id }
        | BrowserControlCommand::Snapshot { surface_id }
        | BrowserControlCommand::Eval { surface_id, .. }
        | BrowserControlCommand::Wait { surface_id, .. }
        | BrowserControlCommand::Click { surface_id, .. }
        | BrowserControlCommand::Dblclick { surface_id, .. }
        | BrowserControlCommand::Type { surface_id, .. }
        | BrowserControlCommand::Fill { surface_id, .. }
        | BrowserControlCommand::Press { surface_id, .. }
        | BrowserControlCommand::Keydown { surface_id, .. }
        | BrowserControlCommand::Keyup { surface_id, .. }
        | BrowserControlCommand::Hover { surface_id, .. }
        | BrowserControlCommand::Focus { surface_id, .. }
        | BrowserControlCommand::Check { surface_id, .. }
        | BrowserControlCommand::Uncheck { surface_id, .. }
        | BrowserControlCommand::Select { surface_id, .. }
        | BrowserControlCommand::Scroll { surface_id, .. }
        | BrowserControlCommand::ScrollIntoView { surface_id, .. }
        | BrowserControlCommand::Get { surface_id, .. }
        | BrowserControlCommand::Is { surface_id, .. }
        | BrowserControlCommand::Screenshot { surface_id, .. } => *surface_id,
    }
}

fn terminal_debug_surface_id(command: &TerminalDebugCommand) -> SurfaceId {
    match command {
        TerminalDebugCommand::IsFocused { surface_id }
        | TerminalDebugCommand::ReadText { surface_id, .. }
        | TerminalDebugCommand::RenderStats { surface_id } => *surface_id,
    }
}

fn trim_terminal_tail(text: String, tail_lines: Option<usize>) -> String {
    let Some(limit) = tail_lines else {
        return text;
    };
    if limit == 0 {
        return String::new();
    }

    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(limit);
    lines[start..].join("\n")
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

fn position_widget(overlay: &Overlay, widget: &Widget, frame: taskers_core::Frame) {
    widget.set_size_request(frame.width.max(1), frame.height.max(1));
    widget.set_hexpand(false);
    widget.set_vexpand(false);
    widget.set_halign(Align::Start);
    widget.set_valign(Align::Start);
    widget.set_margin_start(frame.x.max(0));
    widget.set_margin_top(frame.y.max(0));
    if widget.parent().is_some() {
        widget.queue_allocate();
    } else {
        overlay.add_overlay(widget);
        overlay.set_measure_overlay(widget, false);
        overlay.set_clip_overlay(widget, true);
    }
}

fn detach_from_overlay(overlay: &Overlay, widget: &Widget) {
    if widget.parent().is_some() {
        overlay.remove_overlay(widget);
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
        terminal_plans, trim_terminal_tail, workspace_pan_delta,
    };
    use taskers_domain::PaneKind;
    use taskers_shell_core::{BootstrapModel, SharedCore, ShellDragMode, SurfaceMountSpec};

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

    #[test]
    fn trim_terminal_tail_keeps_requested_suffix() {
        let text = "one\ntwo\nthree\nfour".to_string();
        assert_eq!(trim_terminal_tail(text.clone(), None), text);
        assert_eq!(trim_terminal_tail(text.clone(), Some(2)), "three\nfour");
        assert_eq!(trim_terminal_tail(text, Some(10)), "one\ntwo\nthree\nfour");
    }
}
