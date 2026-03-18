use anyhow::{Context, Result};
use dioxus_desktop::{
    tao::{
        dpi::{LogicalPosition, LogicalSize},
        window::Window,
    },
    wry::{Rect, WebView, WebViewBuilder},
};
use std::{cell::RefCell, collections::HashMap, sync::Arc};
use taskers_core::{Frame, PortalSurfacePlan, ShellSnapshot, SurfaceId, SurfaceKind};

thread_local! {
    static HOST_RUNTIME: RefCell<HostRuntimeState> = RefCell::new(HostRuntimeState::default());
}

#[derive(Default)]
struct HostRuntimeState {
    window: Option<Arc<Window>>,
    browser_surfaces: HashMap<SurfaceId, BrowserSurface>,
}

struct BrowserSurface {
    webview: WebView,
    url: String,
}

pub fn attach_window(window: Arc<Window>) -> Result<()> {
    HOST_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        state.window = Some(window);
        Ok(())
    })
}

pub fn sync_snapshot(snapshot: &ShellSnapshot) -> Result<()> {
    HOST_RUNTIME.with(|slot| {
        let mut state = slot.borrow_mut();
        let Some(window) = state.window.clone() else {
            return Ok(());
        };

        let desired: Vec<_> = snapshot
            .portal
            .panes
            .iter()
            .filter(|pane| pane.kind == SurfaceKind::Browser)
            .cloned()
            .collect();

        let desired_ids: HashMap<SurfaceId, PortalSurfacePlan> = desired
            .iter()
            .cloned()
            .map(|plan| (plan.surface_id, plan))
            .collect();

        state
            .browser_surfaces
            .retain(|surface_id, _| desired_ids.contains_key(surface_id));

        for plan in desired {
            match state.browser_surfaces.get_mut(&plan.surface_id) {
                Some(surface) => surface.sync(&plan)?,
                None => {
                    let surface = BrowserSurface::new(&window, &plan)?;
                    state.browser_surfaces.insert(plan.surface_id, surface);
                }
            }
        }

        Ok(())
    })
}

pub fn terminal_host_status() -> &'static str {
    "Ghostty integration still needs a native portal adapter in the rewrite."
}

impl BrowserSurface {
    fn new(window: &Arc<Window>, plan: &PortalSurfacePlan) -> Result<Self> {
        let url = plan
            .url
            .clone()
            .unwrap_or_else(|| "https://dioxuslabs.com/learn/0.7/".into());

        let webview = WebViewBuilder::new()
            .with_url(&url)
            .with_bounds(rect_from_frame(plan.frame))
            .build_as_child(window.as_ref())
            .with_context(|| format!("failed to create browser surface {}", plan.surface_id.0))?;

        Ok(Self { webview, url })
    }

    fn sync(&mut self, plan: &PortalSurfacePlan) -> Result<()> {
        self.webview
            .set_bounds(rect_from_frame(plan.frame))
            .context("failed to update browser bounds")?;

        if let Some(url) = &plan.url
            && self.url != *url
        {
            self.webview.load_url(url).with_context(|| {
                format!("failed to navigate browser surface {}", plan.surface_id.0)
            })?;
            self.url = url.clone();
        }

        Ok(())
    }
}

fn rect_from_frame(frame: Frame) -> Rect {
    Rect {
        position: LogicalPosition::new(frame.x, frame.y).into(),
        size: LogicalSize::new(frame.width.max(1), frame.height.max(1)).into(),
    }
}
