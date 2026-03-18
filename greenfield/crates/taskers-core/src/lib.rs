use indexmap::IndexMap;
use parking_lot::Mutex;
use std::{fmt, sync::Arc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pane-{}", self.0)
    }
}

impl fmt::Display for SurfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "surface-{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    Terminal,
    Browser,
}

impl SurfaceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Browser => "Browser",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelSize {
    pub width: i32,
    pub height: i32,
}

impl PixelSize {
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Frame {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn inset_top(self, amount: i32) -> Self {
        let clamped = amount.clamp(0, self.height.saturating_sub(1));
        Self {
            x: self.x,
            y: self.y + clamped,
            width: self.width,
            height: (self.height - clamped).max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub sidebar_width: i32,
    pub toolbar_height: i32,
    pub workspace_padding: i32,
    pub split_gap: i32,
    pub pane_header_height: i32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            sidebar_width: 248,
            toolbar_height: 64,
            workspace_padding: 16,
            split_gap: 12,
            pane_header_height: 38,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSnapshot {
    pub id: SurfaceId,
    pub kind: SurfaceKind,
    pub title: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub active: bool,
    pub surface: SurfaceSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LayoutNodeSnapshot {
    Pane(PaneSnapshot),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<LayoutNodeSnapshot>,
        second: Box<LayoutNodeSnapshot>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalSurfacePlan {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub kind: SurfaceKind,
    pub title: String,
    pub url: Option<String>,
    pub frame: Frame,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePortalPlan {
    pub window: Frame,
    pub content: Frame,
    pub panes: Vec<PortalSurfacePlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSnapshot {
    pub revision: u64,
    pub workspace_title: String,
    pub workspace_count: usize,
    pub active_pane: PaneId,
    pub layout: LayoutNodeSnapshot,
    pub portal: SurfacePortalPlan,
    pub metrics: LayoutMetrics,
}

#[derive(Debug, Clone)]
struct SurfaceRecord {
    id: SurfaceId,
    kind: SurfaceKind,
    title: String,
    url: Option<String>,
}

#[derive(Debug, Clone)]
struct PaneRecord {
    id: PaneId,
    surface: SurfaceRecord,
}

#[derive(Debug, Clone)]
enum LayoutNode {
    Leaf(PaneId),
    Split {
        axis: SplitAxis,
        ratio_millis: u16,
        first: Box<LayoutNode>,
        second: Box<LayoutNode>,
    },
}

impl LayoutNode {
    fn split_leaf(
        &mut self,
        target: PaneId,
        axis: SplitAxis,
        new_pane: PaneId,
        ratio_millis: u16,
    ) -> bool {
        match self {
            Self::Leaf(existing) if *existing == target => {
                let old = *existing;
                *self = Self::Split {
                    axis,
                    ratio_millis,
                    first: Box::new(Self::Leaf(old)),
                    second: Box::new(Self::Leaf(new_pane)),
                };
                true
            }
            Self::Split { first, second, .. } => {
                first.split_leaf(target, axis, new_pane, ratio_millis)
                    || second.split_leaf(target, axis, new_pane, ratio_millis)
            }
            Self::Leaf(_) => false,
        }
    }
}

#[derive(Debug, Clone)]
struct AppModel {
    workspace_title: String,
    active_pane: PaneId,
    panes: IndexMap<PaneId, PaneRecord>,
    layout: LayoutNode,
    window_size: PixelSize,
}

#[derive(Debug)]
struct TaskersCore {
    next_id: u64,
    revision: u64,
    metrics: LayoutMetrics,
    model: AppModel,
}

impl TaskersCore {
    fn demo() -> Self {
        let metrics = LayoutMetrics::default();
        let mut next_id = 1;
        let first_pane = PaneId(next_id);
        next_id += 1;
        let first_surface = SurfaceId(next_id);
        next_id += 1;

        let surface = SurfaceRecord {
            id: first_surface,
            kind: SurfaceKind::Terminal,
            title: "Agent shell".into(),
            url: None,
        };
        let pane = PaneRecord {
            id: first_pane,
            surface,
        };

        let mut panes = IndexMap::new();
        panes.insert(first_pane, pane);

        Self {
            next_id,
            revision: 1,
            metrics,
            model: AppModel {
                workspace_title: "Main".into(),
                active_pane: first_pane,
                panes,
                layout: LayoutNode::Leaf(first_pane),
                window_size: PixelSize::new(1440, 900),
            },
        }
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn snapshot(&self) -> ShellSnapshot {
        let layout = self.snapshot_layout(&self.model.layout);
        let content = self.content_frame();
        let portal = SurfacePortalPlan {
            window: Frame::new(
                0,
                0,
                self.model.window_size.width,
                self.model.window_size.height,
            ),
            content,
            panes: self.collect_surface_plans(&self.model.layout, content),
        };

        ShellSnapshot {
            revision: self.revision,
            workspace_title: self.model.workspace_title.clone(),
            workspace_count: 1,
            active_pane: self.model.active_pane,
            layout,
            portal,
            metrics: self.metrics,
        }
    }

    fn snapshot_layout(&self, node: &LayoutNode) -> LayoutNodeSnapshot {
        match node {
            LayoutNode::Leaf(pane_id) => {
                let pane = self
                    .model
                    .panes
                    .get(pane_id)
                    .expect("layout pane should exist");
                LayoutNodeSnapshot::Pane(PaneSnapshot {
                    id: pane.id,
                    active: pane.id == self.model.active_pane,
                    surface: SurfaceSnapshot {
                        id: pane.surface.id,
                        kind: pane.surface.kind,
                        title: pane.surface.title.clone(),
                        url: pane.surface.url.clone(),
                    },
                })
            }
            LayoutNode::Split {
                axis,
                ratio_millis,
                first,
                second,
            } => LayoutNodeSnapshot::Split {
                axis: *axis,
                ratio: f32::from(*ratio_millis) / 1000.0,
                first: Box::new(self.snapshot_layout(first)),
                second: Box::new(self.snapshot_layout(second)),
            },
        }
    }

    fn content_frame(&self) -> Frame {
        let metrics = self.metrics;
        let padding = metrics.workspace_padding;
        let x = metrics.sidebar_width + padding;
        let y = metrics.toolbar_height + padding;
        let width = (self.model.window_size.width - metrics.sidebar_width - (padding * 2)).max(320);
        let height =
            (self.model.window_size.height - metrics.toolbar_height - (padding * 2)).max(240);
        Frame::new(x, y, width, height)
    }

    fn collect_surface_plans(&self, node: &LayoutNode, frame: Frame) -> Vec<PortalSurfacePlan> {
        let mut panes = Vec::new();
        self.collect_surface_plans_into(node, frame, &mut panes);
        panes
    }

    fn collect_surface_plans_into(
        &self,
        node: &LayoutNode,
        frame: Frame,
        out: &mut Vec<PortalSurfacePlan>,
    ) {
        match node {
            LayoutNode::Leaf(pane_id) => {
                if let Some(pane) = self.model.panes.get(pane_id) {
                    out.push(PortalSurfacePlan {
                        pane_id: pane.id,
                        surface_id: pane.surface.id,
                        kind: pane.surface.kind,
                        title: pane.surface.title.clone(),
                        url: pane.surface.url.clone(),
                        frame: frame.inset_top(self.metrics.pane_header_height),
                    });
                }
            }
            LayoutNode::Split {
                axis,
                ratio_millis,
                first,
                second,
            } => {
                let (first_frame, second_frame) =
                    split_frame(frame, *axis, *ratio_millis, self.metrics.split_gap);
                self.collect_surface_plans_into(first, first_frame, out);
                self.collect_surface_plans_into(second, second_frame, out);
            }
        }
    }

    fn split_active(&mut self, kind: SurfaceKind, axis: SplitAxis) {
        let new_pane = PaneId(self.next_id);
        self.next_id += 1;
        let new_surface = SurfaceId(self.next_id);
        self.next_id += 1;

        let title = match kind {
            SurfaceKind::Terminal => format!("Task {}", new_pane.0),
            SurfaceKind::Browser => "Browser".into(),
        };
        let url = match kind {
            SurfaceKind::Browser => Some("https://dioxuslabs.com/learn/0.7/".into()),
            SurfaceKind::Terminal => None,
        };

        let pane = PaneRecord {
            id: new_pane,
            surface: SurfaceRecord {
                id: new_surface,
                kind,
                title,
                url,
            },
        };
        self.model.panes.insert(new_pane, pane);

        if self
            .model
            .layout
            .split_leaf(self.model.active_pane, axis, new_pane, 500)
        {
            self.model.active_pane = new_pane;
            self.revision += 1;
        }
    }

    fn focus_pane(&mut self, pane_id: PaneId) {
        if self.model.panes.contains_key(&pane_id) && self.model.active_pane != pane_id {
            self.model.active_pane = pane_id;
            self.revision += 1;
        }
    }

    fn set_window_size(&mut self, size: PixelSize) {
        if self.model.window_size != size {
            self.model.window_size = size;
            self.revision += 1;
        }
    }
}

fn split_frame(frame: Frame, axis: SplitAxis, ratio_millis: u16, gap: i32) -> (Frame, Frame) {
    let ratio = f32::from(ratio_millis.clamp(100, 900)) / 1000.0;

    match axis {
        SplitAxis::Horizontal => {
            let available = (frame.width - gap).max(2);
            let first_width = ((available as f32) * ratio).round() as i32;
            let second_width = (available - first_width).max(1);
            let first_width = first_width.max(1);

            (
                Frame::new(frame.x, frame.y, first_width, frame.height),
                Frame::new(
                    frame.x + first_width + gap,
                    frame.y,
                    second_width,
                    frame.height,
                ),
            )
        }
        SplitAxis::Vertical => {
            let available = (frame.height - gap).max(2);
            let first_height = ((available as f32) * ratio).round() as i32;
            let second_height = (available - first_height).max(1);
            let first_height = first_height.max(1);

            (
                Frame::new(frame.x, frame.y, frame.width, first_height),
                Frame::new(
                    frame.x,
                    frame.y + first_height + gap,
                    frame.width,
                    second_height,
                ),
            )
        }
    }
}

#[derive(Clone)]
pub struct SharedCore {
    inner: Arc<Mutex<TaskersCore>>,
}

impl SharedCore {
    pub fn demo() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TaskersCore::demo())),
        }
    }

    pub fn revision(&self) -> u64 {
        self.inner.lock().revision()
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.inner.lock().snapshot()
    }

    pub fn set_window_size(&self, size: PixelSize) {
        self.inner.lock().set_window_size(size);
    }

    pub fn split_with_browser(&self) {
        self.inner
            .lock()
            .split_active(SurfaceKind::Browser, SplitAxis::Horizontal);
    }

    pub fn split_with_terminal(&self) {
        self.inner
            .lock()
            .split_active(SurfaceKind::Terminal, SplitAxis::Vertical);
    }

    pub fn focus_pane(&self, pane_id: PaneId) {
        self.inner.lock().focus_pane(pane_id);
    }
}

impl PartialEq for SharedCore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SharedCore {}
