use anyhow::Result;
use dioxus_desktop::tao::window::Window;
use std::{
    cell::RefCell,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use taskers_core::{HostEvent, PaneId, RuntimeCapability, ShellSnapshot, SurfaceId};

type HostEventSink = Arc<dyn Fn(HostEvent) + Send + Sync + 'static>;
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
    pub pane_id: Option<PaneId>,
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

    pub fn with_pane(mut self, pane_id: PaneId) -> Self {
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

thread_local! {
    static HOST_RUNTIME: RefCell<HostRuntimeState> = RefCell::new(HostRuntimeState::default());
}

#[derive(Default)]
struct HostRuntimeState {
    window: Option<Arc<Window>>,
    event_sink: Option<HostEventSink>,
    diagnostics: Option<DiagnosticsSink>,
    #[cfg(target_os = "linux")]
    linux: linux::LinuxHostRuntime,
}

pub fn attach_window(
    window: Arc<Window>,
    event_sink: HostEventSink,
    diagnostics: Option<DiagnosticsSink>,
) -> Result<()> {
    HOST_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.window = Some(window);
        state.event_sink = Some(event_sink);
        state.diagnostics = diagnostics.clone();
        emit_diagnostic(
            diagnostics.as_ref(),
            DiagnosticRecord::new(DiagnosticCategory::Window, None, "host window attached"),
        );
        Ok(())
    })
}

pub fn sync_snapshot(snapshot: &ShellSnapshot) -> Result<()> {
    HOST_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let Some(window) = state.window.clone() else {
            return Ok(());
        };

        #[cfg(target_os = "linux")]
        {
            if let Some(event_sink) = state.event_sink.clone() {
                let diagnostics = state.diagnostics.clone();
                return state
                    .linux
                    .sync_snapshot(&window, snapshot, &event_sink, diagnostics.as_ref());
            }
        }

        let _ = snapshot;
        Ok(())
    })
}

pub fn terminal_host_capability() -> RuntimeCapability {
    #[cfg(target_os = "linux")]
    {
        RuntimeCapability::Fallback {
            message: "Linux Ghostty embedding is still blocked here: the existing bridge exposes GTK4 widgets, but the Dioxus desktop host is GTK3.".into(),
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        RuntimeCapability::Unavailable {
            message: "This greenfield checkpoint only wires the Linux host runtime.".into(),
        }
    }
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

#[cfg(target_os = "linux")]
mod linux {
    use super::{DiagnosticCategory, DiagnosticRecord, DiagnosticsSink, HostEventSink, emit_diagnostic};
    use anyhow::{Context, Result};
    use dioxus_desktop::{
        tao::{
            dpi::{LogicalPosition, LogicalSize},
            platform::unix::WindowExtUnix,
            window::Window,
        },
        wry::{PageLoadEvent, Rect, WebView, WebViewBuilder, WebViewBuilderExtUnix, WebViewExtUnix},
    };
    use gtk::{Fixed, Overlay, Widget, glib::Propagation, prelude::*};
    use std::{collections::{HashMap, HashSet}, sync::Arc};
    use taskers_core::{BrowserMountSpec, HostEvent, PortalSurfacePlan, ShellSnapshot, SurfaceId, SurfaceMountSpec};

    #[derive(Default)]
    pub struct LinuxHostRuntime {
        portal_overlay: Option<Overlay>,
        portal_fixed: Option<Fixed>,
        browser_surfaces: HashMap<SurfaceId, BrowserSurface>,
    }

    impl LinuxHostRuntime {
        pub fn sync_snapshot(
            &mut self,
            window: &Arc<Window>,
            snapshot: &ShellSnapshot,
            event_sink: &HostEventSink,
            diagnostics: Option<&DiagnosticsSink>,
        ) -> Result<()> {
            emit_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Sync,
                    Some(snapshot.revision),
                    format!("host sync start panes={}", snapshot.portal.panes.len()),
                ),
            );

            let Some(fixed) = self.ensure_portal_layer(window, diagnostics)? else {
                return Ok(());
            };

            let desired: Vec<_> = snapshot
                .portal
                .panes
                .iter()
                .filter(|pane| matches!(pane.mount, SurfaceMountSpec::Browser(_)))
                .cloned()
                .collect();

            let diff = browser_surface_diff(
                self.browser_surfaces.keys().copied(),
                desired.iter().map(|plan| plan.surface_id),
            );

            for surface_id in diff.removed {
                self.browser_surfaces.remove(&surface_id);
                emit_diagnostic(
                    diagnostics,
                    DiagnosticRecord::new(
                        DiagnosticCategory::SurfaceLifecycle,
                        Some(snapshot.revision),
                        "browser surface removed",
                    )
                    .with_surface(surface_id),
                );
            }

            for plan in desired {
                match self.browser_surfaces.get_mut(&plan.surface_id) {
                    Some(surface) => surface.sync(&plan, snapshot.revision, diagnostics)?,
                    None => {
                        let surface =
                            BrowserSurface::new(&fixed, &plan, snapshot.revision, event_sink, diagnostics)?;
                        self.browser_surfaces.insert(plan.surface_id, surface);
                    }
                }
            }

            Ok(())
        }

        fn ensure_portal_layer(
            &mut self,
            window: &Arc<Window>,
            diagnostics: Option<&DiagnosticsSink>,
        ) -> Result<Option<Fixed>> {
            if let Some(fixed) = &self.portal_fixed {
                return Ok(Some(fixed.clone()));
            }

            let Some(vbox) = window.default_vbox() else {
                return Ok(None);
            };
            let children = vbox.children();
            let Some(webview_child) = children.into_iter().next_back() else {
                return Ok(None);
            };

            let overlay = Overlay::new();
            overlay.set_hexpand(true);
            overlay.set_vexpand(true);

            let fixed = Fixed::new();
            fixed.set_hexpand(true);
            fixed.set_vexpand(true);

            vbox.remove(&webview_child);
            overlay.add(&webview_child);
            overlay.add_overlay(&fixed);
            vbox.pack_start(&overlay, true, true, 0);
            overlay.show_all();

            self.portal_overlay = Some(overlay);
            self.portal_fixed = Some(fixed.clone());
            emit_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::Window,
                    None,
                    "created linux portal overlay layer",
                ),
            );
            Ok(Some(fixed))
        }
    }

    struct BrowserSurface {
        webview: WebView,
        url: String,
    }

    impl BrowserSurface {
        fn new(
            fixed: &Fixed,
            plan: &PortalSurfacePlan,
            revision: u64,
            event_sink: &HostEventSink,
            diagnostics: Option<&DiagnosticsSink>,
        ) -> Result<Self> {
            let BrowserMountSpec { url } = browser_spec(plan)?.clone();
            let surface_id = plan.surface_id;
            let pane_id = plan.pane_id;

            let title_sink = event_sink.clone();
            let title_diag = diagnostics.cloned();
            let url_sink = event_sink.clone();
            let url_diag = diagnostics.cloned();
            let webview = WebViewBuilder::new()
                .with_url(&url)
                .with_bounds(rect_from_frame(plan.frame))
                .with_document_title_changed_handler(move |title| {
                    emit_diagnostic(
                        title_diag.as_ref(),
                        DiagnosticRecord::new(
                            DiagnosticCategory::BrowserMetadata,
                            None,
                            format!("browser title observed: {title}"),
                        )
                        .with_pane(pane_id)
                        .with_surface(surface_id),
                    );
                    (title_sink)(HostEvent::SurfaceTitleChanged { surface_id, title });
                })
                .with_on_page_load_handler(move |event, url| {
                    if matches!(event, PageLoadEvent::Finished) {
                        emit_diagnostic(
                            url_diag.as_ref(),
                            DiagnosticRecord::new(
                                DiagnosticCategory::BrowserMetadata,
                                None,
                                format!("browser url observed: {url}"),
                            )
                            .with_pane(pane_id)
                            .with_surface(surface_id),
                        );
                        (url_sink)(HostEvent::SurfaceUrlChanged { surface_id, url });
                    }
                })
                .build_gtk(fixed)
                .with_context(|| format!("failed to create browser surface {}", plan.surface_id.0))?;

            let widget = webview.webview();
            let focus_sink = event_sink.clone();
            let focus_diag = diagnostics.cloned();
            widget.connect_focus_in_event(move |_, _| {
                emit_diagnostic(
                    focus_diag.as_ref(),
                    DiagnosticRecord::new(
                        DiagnosticCategory::HostEvent,
                        None,
                        "browser focus event received",
                    )
                    .with_pane(pane_id)
                    .with_surface(surface_id),
                );
                (focus_sink)(HostEvent::PaneFocused { pane_id });
                Propagation::Proceed
            });

            if plan.active {
                let _ = webview.focus();
            }

            emit_diagnostic(
                diagnostics,
                DiagnosticRecord::new(
                    DiagnosticCategory::SurfaceLifecycle,
                    Some(revision),
                    "browser surface created",
                )
                .with_pane(plan.pane_id)
                .with_surface(plan.surface_id),
            );

            Ok(Self { webview, url })
        }

        fn sync(
            &mut self,
            plan: &PortalSurfacePlan,
            revision: u64,
            diagnostics: Option<&DiagnosticsSink>,
        ) -> Result<()> {
            self.webview
                .set_bounds(rect_from_frame(plan.frame))
                .context("failed to update browser bounds")?;

            let BrowserMountSpec { url } = browser_spec(plan)?;
            if self.url != *url {
                self.webview
                    .load_url(url)
                    .with_context(|| format!("failed to navigate browser surface {}", plan.surface_id.0))?;
                self.url = url.clone();
            }

            if plan.active {
                let _ = self.webview.focus();
            }

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
    }

    #[derive(Debug, PartialEq, Eq)]
    struct BrowserSurfaceDiff {
        removed: Vec<SurfaceId>,
    }

    fn browser_surface_diff(
        existing: impl IntoIterator<Item = SurfaceId>,
        desired: impl IntoIterator<Item = SurfaceId>,
    ) -> BrowserSurfaceDiff {
        let desired = desired.into_iter().collect::<HashSet<_>>();
        let removed = existing
            .into_iter()
            .filter(|surface_id| !desired.contains(surface_id))
            .collect::<Vec<_>>();

        BrowserSurfaceDiff { removed }
    }

    fn browser_spec(plan: &PortalSurfacePlan) -> Result<&BrowserMountSpec> {
        match &plan.mount {
            SurfaceMountSpec::Browser(spec) => Ok(spec),
            SurfaceMountSpec::Terminal(_) => anyhow::bail!(
                "surface {} is not a browser mount",
                plan.surface_id.0
            ),
        }
    }

    fn rect_from_frame(frame: taskers_core::Frame) -> Rect {
        Rect {
            position: LogicalPosition::new(frame.x, frame.y).into(),
            size: LogicalSize::new(frame.width.max(1), frame.height.max(1)).into(),
        }
    }

    #[allow(dead_code)]
    fn _widget_debug_name(widget: &Widget) -> &'static str {
        widget.type_().name()
    }

    #[cfg(test)]
    mod tests {
        use super::{BrowserSurfaceDiff, browser_surface_diff};
        use taskers_core::SurfaceId;

        #[test]
        fn browser_surface_diff_returns_removed_ids() {
            let diff =
                browser_surface_diff([SurfaceId(1), SurfaceId(2), SurfaceId(3)], [SurfaceId(2)]);

            assert_eq!(
                diff,
                BrowserSurfaceDiff {
                    removed: vec![SurfaceId(1), SurfaceId(3)],
                }
            );
        }

        #[test]
        fn browser_surface_diff_keeps_matching_ids() {
            let diff = browser_surface_diff([SurfaceId(7)], [SurfaceId(7)]);
            assert_eq!(diff, BrowserSurfaceDiff { removed: vec![] });
        }
    }
}
