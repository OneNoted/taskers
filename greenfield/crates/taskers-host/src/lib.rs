use anyhow::Result;
use dioxus_desktop::tao::window::Window;
use std::{cell::RefCell, sync::Arc};
use taskers_core::{HostEvent, RuntimeCapability, ShellSnapshot};

type HostEventSink = Arc<dyn Fn(HostEvent) + 'static>;

thread_local! {
    static HOST_RUNTIME: RefCell<HostRuntimeState> = RefCell::new(HostRuntimeState::default());
}

#[derive(Default)]
struct HostRuntimeState {
    window: Option<Arc<Window>>,
    event_sink: Option<HostEventSink>,
    #[cfg(target_os = "linux")]
    linux: linux::LinuxHostRuntime,
}

pub fn attach_window(window: Arc<Window>, event_sink: HostEventSink) -> Result<()> {
    HOST_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.window = Some(window);
        state.event_sink = Some(event_sink);
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
                return state.linux.sync_snapshot(&window, snapshot, &event_sink);
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

#[cfg(target_os = "linux")]
mod linux {
    use super::HostEventSink;
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
    use std::{collections::HashMap, sync::Arc};
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
        ) -> Result<()> {
            let Some(fixed) = self.ensure_portal_layer(window)? else {
                return Ok(());
            };

            let desired: Vec<_> = snapshot
                .portal
                .panes
                .iter()
                .filter(|pane| matches!(pane.mount, SurfaceMountSpec::Browser(_)))
                .cloned()
                .collect();

            let desired_ids = desired
                .iter()
                .map(|plan| plan.surface_id)
                .collect::<std::collections::HashSet<_>>();

            self.browser_surfaces
                .retain(|surface_id, _| desired_ids.contains(surface_id));

            for plan in desired {
                match self.browser_surfaces.get_mut(&plan.surface_id) {
                    Some(surface) => surface.sync(&plan)?,
                    None => {
                        let surface = BrowserSurface::new(&fixed, &plan, event_sink)?;
                        self.browser_surfaces.insert(plan.surface_id, surface);
                    }
                }
            }

            Ok(())
        }

        fn ensure_portal_layer(&mut self, window: &Arc<Window>) -> Result<Option<Fixed>> {
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
            Ok(Some(fixed))
        }
    }

    struct BrowserSurface {
        webview: WebView,
        url: String,
    }

    impl BrowserSurface {
        fn new(fixed: &Fixed, plan: &PortalSurfacePlan, event_sink: &HostEventSink) -> Result<Self> {
            let BrowserMountSpec { url } = browser_spec(plan)?.clone();
            let surface_id = plan.surface_id;
            let pane_id = plan.pane_id;

            let title_sink = event_sink.clone();
            let url_sink = event_sink.clone();
            let webview = WebViewBuilder::new()
                .with_url(&url)
                .with_bounds(rect_from_frame(plan.frame))
                .with_document_title_changed_handler(move |title| {
                    (title_sink)(HostEvent::SurfaceTitleChanged { surface_id, title });
                })
                .with_on_page_load_handler(move |event, url| {
                    if matches!(event, PageLoadEvent::Finished) {
                        (url_sink)(HostEvent::SurfaceUrlChanged { surface_id, url });
                    }
                })
                .build_gtk(fixed)
                .with_context(|| format!("failed to create browser surface {}", plan.surface_id.0))?;

            let widget = webview.webview();
            let focus_sink = event_sink.clone();
            widget.connect_focus_in_event(move |_, _| {
                (focus_sink)(HostEvent::PaneFocused { pane_id });
                Propagation::Proceed
            });

            if plan.active {
                let _ = webview.focus();
            }

            Ok(Self { webview, url })
        }

        fn sync(&mut self, plan: &PortalSurfacePlan) -> Result<()> {
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

            Ok(())
        }
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
}
