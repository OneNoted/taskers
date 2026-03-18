use indexmap::IndexMap;
use parking_lot::Mutex;
use std::{collections::BTreeMap, fmt, sync::Arc};
use tokio::sync::watch;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCapability {
    Ready,
    Fallback { message: String },
    Unavailable { message: String },
}

impl RuntimeCapability {
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::Fallback { message } | Self::Unavailable { message } => Some(message),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Fallback { .. } => "Fallback",
            Self::Unavailable { .. } => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub ghostty_runtime: RuntimeCapability,
    pub shell_integration: RuntimeCapability,
    pub terminal_host: RuntimeCapability,
}

impl Default for RuntimeStatus {
    fn default() -> Self {
        let unavailable = || RuntimeCapability::Unavailable {
            message: "Not configured yet.".into(),
        };
        Self {
            ghostty_runtime: unavailable(),
            shell_integration: unavailable(),
            terminal_host: unavailable(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalDefaults {
    pub cols: u16,
    pub rows: u16,
    pub command_argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl Default for TerminalDefaults {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 40,
            command_argv: vec!["/bin/sh".into()],
            env: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootstrapModel {
    pub runtime_status: RuntimeStatus,
    pub terminal_defaults: TerminalDefaults,
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
    pub cwd: Option<String>,
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
pub struct BrowserMountSpec {
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalMountSpec {
    pub title: String,
    pub cwd: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub command_argv: Vec<String>,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceMountSpec {
    Browser(BrowserMountSpec),
    Terminal(TerminalMountSpec),
}

impl SurfaceMountSpec {
    pub fn kind(&self) -> SurfaceKind {
        match self {
            Self::Browser(_) => SurfaceKind::Browser,
            Self::Terminal(_) => SurfaceKind::Terminal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalSurfacePlan {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub active: bool,
    pub frame: Frame,
    pub mount: SurfaceMountSpec,
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
    pub runtime_status: RuntimeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    PaneFocused { pane_id: PaneId },
    SurfaceClosed { pane_id: PaneId, surface_id: SurfaceId },
    SurfaceTitleChanged { surface_id: SurfaceId, title: String },
    SurfaceUrlChanged { surface_id: SurfaceId, url: String },
    SurfaceCwdChanged { surface_id: SurfaceId, cwd: String },
}

#[derive(Debug, Clone)]
struct SurfaceRecord {
    id: SurfaceId,
    kind: SurfaceKind,
    title: String,
    url: Option<String>,
    cwd: Option<String>,
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

    fn remove_leaf(self, target: PaneId) -> Option<Self> {
        match self {
            Self::Leaf(existing) if existing == target => None,
            Self::Leaf(existing) => Some(Self::Leaf(existing)),
            Self::Split {
                axis,
                ratio_millis,
                first,
                second,
            } => {
                let first = first.remove_leaf(target);
                let second = second.remove_leaf(target);
                match (first, second) {
                    (Some(first), Some(second)) => Some(Self::Split {
                        axis,
                        ratio_millis,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    (Some(first), None) => Some(first),
                    (None, Some(second)) => Some(second),
                    (None, None) => None,
                }
            }
        }
    }

    fn first_leaf_id(&self) -> PaneId {
        match self {
            Self::Leaf(pane_id) => *pane_id,
            Self::Split { first, .. } => first.first_leaf_id(),
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
    runtime_status: RuntimeStatus,
    terminal_defaults: TerminalDefaults,
}

impl TaskersCore {
    fn with_bootstrap(bootstrap: BootstrapModel) -> Self {
        let metrics = LayoutMetrics::default();
        let mut core = Self {
            next_id: 1,
            revision: 1,
            metrics,
            model: AppModel {
                workspace_title: "Main".into(),
                active_pane: PaneId(0),
                panes: IndexMap::new(),
                layout: LayoutNode::Leaf(PaneId(0)),
                window_size: PixelSize::new(1440, 900),
            },
            runtime_status: bootstrap.runtime_status,
            terminal_defaults: bootstrap.terminal_defaults,
        };

        let pane = core.make_surface(SurfaceKind::Terminal, "Agent shell".into(), None, None);
        core.model.active_pane = pane.id;
        core.model.layout = LayoutNode::Leaf(pane.id);
        core.model.panes.insert(pane.id, pane);
        core
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
            runtime_status: self.runtime_status.clone(),
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
                        cwd: pane.surface.cwd.clone(),
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
                        active: pane.id == self.model.active_pane,
                        frame: frame.inset_top(self.metrics.pane_header_height),
                        mount: self.mount_spec_for(pane),
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

    fn mount_spec_for(&self, pane: &PaneRecord) -> SurfaceMountSpec {
        match pane.surface.kind {
            SurfaceKind::Browser => SurfaceMountSpec::Browser(BrowserMountSpec {
                url: pane
                    .surface
                    .url
                    .clone()
                    .unwrap_or_else(|| "https://dioxuslabs.com/learn/0.7/".into()),
            }),
            SurfaceKind::Terminal => {
                let mut env = self.terminal_defaults.env.clone();
                env.insert("TASKERS_PANE_ID".into(), pane.id.to_string());
                env.insert("TASKERS_SURFACE_ID".into(), pane.surface.id.to_string());
                env.insert("TASKERS_WORKSPACE_ID".into(), "main".into());
                SurfaceMountSpec::Terminal(TerminalMountSpec {
                    title: pane.surface.title.clone(),
                    cwd: pane.surface.cwd.clone(),
                    cols: self.terminal_defaults.cols,
                    rows: self.terminal_defaults.rows,
                    command_argv: self.terminal_defaults.command_argv.clone(),
                    env,
                })
            }
        }
    }

    fn split_active(&mut self, kind: SurfaceKind, axis: SplitAxis) -> bool {
        let pane = match kind {
            SurfaceKind::Terminal => self.make_surface(kind, self.next_terminal_title(), None, None),
            SurfaceKind::Browser => self.make_surface(
                kind,
                "Browser".into(),
                Some("https://dioxuslabs.com/learn/0.7/".into()),
                None,
            ),
        };
        let pane_id = pane.id;
        self.model.panes.insert(pane.id, pane);

        if self
            .model
            .layout
            .split_leaf(self.model.active_pane, axis, pane_id, 500)
        {
            self.model.active_pane = pane_id;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        if self.model.panes.contains_key(&pane_id) && self.model.active_pane != pane_id {
            self.model.active_pane = pane_id;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn set_window_size(&mut self, size: PixelSize) -> bool {
        if self.model.window_size != size {
            self.model.window_size = size;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn apply_host_event(&mut self, event: HostEvent) -> bool {
        match event {
            HostEvent::PaneFocused { pane_id } => self.focus_pane(pane_id),
            HostEvent::SurfaceClosed {
                pane_id,
                surface_id,
            } => self.close_surface(pane_id, surface_id),
            HostEvent::SurfaceTitleChanged { surface_id, title } => {
                self.update_surface(surface_id, |surface| {
                    if surface.title != title {
                        surface.title = title.clone();
                        true
                    } else {
                        false
                    }
                })
            }
            HostEvent::SurfaceUrlChanged { surface_id, url } => {
                self.update_surface(surface_id, |surface| {
                    if surface.url.as_deref() != Some(url.as_str()) {
                        surface.url = Some(url.clone());
                        true
                    } else {
                        false
                    }
                })
            }
            HostEvent::SurfaceCwdChanged { surface_id, cwd } => {
                self.update_surface(surface_id, |surface| {
                    if surface.cwd.as_deref() != Some(cwd.as_str()) {
                        surface.cwd = Some(cwd.clone());
                        true
                    } else {
                        false
                    }
                })
            }
        }
    }

    fn close_surface(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let Some(pane) = self.model.panes.get(&pane_id) else {
            return false;
        };
        if pane.surface.id != surface_id {
            return false;
        }

        self.model.panes.shift_remove(&pane_id);
        self.model.layout = match self.model.layout.clone().remove_leaf(pane_id) {
            Some(layout) => layout,
            None => {
                let replacement = self.make_surface(
                    SurfaceKind::Terminal,
                    "Agent shell".into(),
                    None,
                    None,
                );
                let replacement_id = replacement.id;
                self.model.panes.insert(replacement_id, replacement);
                LayoutNode::Leaf(replacement_id)
            }
        };

        if !self.model.panes.contains_key(&self.model.active_pane) {
            self.model.active_pane = self.model.layout.first_leaf_id();
        }

        self.revision += 1;
        true
    }

    fn update_surface(
        &mut self,
        surface_id: SurfaceId,
        mut update: impl FnMut(&mut SurfaceRecord) -> bool,
    ) -> bool {
        for pane in self.model.panes.values_mut() {
            if pane.surface.id == surface_id && update(&mut pane.surface) {
                self.revision += 1;
                return true;
            }
        }
        false
    }

    fn next_terminal_title(&self) -> String {
        format!("Task {}", self.next_id)
    }

    fn make_surface(
        &mut self,
        kind: SurfaceKind,
        title: String,
        url: Option<String>,
        cwd: Option<String>,
    ) -> PaneRecord {
        let pane_id = PaneId(self.next_id);
        self.next_id += 1;
        let surface_id = SurfaceId(self.next_id);
        self.next_id += 1;
        PaneRecord {
            id: pane_id,
            surface: SurfaceRecord {
                id: surface_id,
                kind,
                title,
                url,
                cwd,
            },
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
    revisions: watch::Sender<u64>,
}

impl SharedCore {
    pub fn bootstrap(bootstrap: BootstrapModel) -> Self {
        let core = TaskersCore::with_bootstrap(bootstrap);
        let (revisions, _) = watch::channel(core.revision());
        Self {
            inner: Arc::new(Mutex::new(core)),
            revisions,
        }
    }

    pub fn demo() -> Self {
        Self::bootstrap(BootstrapModel::default())
    }

    pub fn revision(&self) -> u64 {
        self.inner.lock().revision()
    }

    pub fn subscribe_revisions(&self) -> watch::Receiver<u64> {
        self.revisions.subscribe()
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.inner.lock().snapshot()
    }

    pub fn set_window_size(&self, size: PixelSize) {
        self.mutate(|core| core.set_window_size(size));
    }

    pub fn split_with_browser(&self) {
        self.mutate(|core| core.split_active(SurfaceKind::Browser, SplitAxis::Horizontal));
    }

    pub fn split_with_terminal(&self) {
        self.mutate(|core| core.split_active(SurfaceKind::Terminal, SplitAxis::Vertical));
    }

    pub fn focus_pane(&self, pane_id: PaneId) {
        self.mutate(|core| core.focus_pane(pane_id));
    }

    pub fn apply_host_event(&self, event: HostEvent) {
        self.mutate(|core| core.apply_host_event(event));
    }

    fn mutate(&self, update: impl FnOnce(&mut TaskersCore) -> bool) {
        let mut core = self.inner.lock();
        if update(&mut core) {
            let _ = self.revisions.send(core.revision());
        }
    }
}

impl PartialEq for SharedCore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SharedCore {}

#[cfg(test)]
mod tests {
    use super::{
        BootstrapModel, BrowserMountSpec, HostEvent, RuntimeCapability, RuntimeStatus, SharedCore,
        SurfaceMountSpec, SurfaceKind, TerminalDefaults,
    };
    use std::collections::BTreeMap;

    fn bootstrap() -> BootstrapModel {
        let mut env = BTreeMap::new();
        env.insert("TASKERS_SOCKET".into(), "/tmp/taskers.sock".into());
        BootstrapModel {
            runtime_status: RuntimeStatus {
                ghostty_runtime: RuntimeCapability::Ready,
                shell_integration: RuntimeCapability::Ready,
                terminal_host: RuntimeCapability::Fallback {
                    message: "GTK4 Ghostty bridge cannot mount into the GTK3 Dioxus host yet."
                        .into(),
                },
            },
            terminal_defaults: TerminalDefaults {
                cols: 132,
                rows: 48,
                command_argv: vec!["/bin/zsh".into(), "-i".into()],
                env,
            },
        }
    }

    #[test]
    fn terminal_mount_spec_inherits_shell_defaults() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let terminal = snapshot
            .portal
            .panes
            .into_iter()
            .find(|pane| pane.mount.kind() == SurfaceKind::Terminal)
            .expect("terminal surface plan");

        match terminal.mount {
            SurfaceMountSpec::Terminal(spec) => {
                assert_eq!(spec.command_argv, vec!["/bin/zsh", "-i"]);
                assert_eq!(spec.cols, 132);
                assert_eq!(spec.rows, 48);
                assert_eq!(
                    spec.env.get("TASKERS_SOCKET").map(String::as_str),
                    Some("/tmp/taskers.sock")
                );
                assert_eq!(
                    spec.env.get("TASKERS_PANE_ID").map(String::as_str),
                    Some(terminal.pane_id.to_string().as_str())
                );
            }
            SurfaceMountSpec::Browser(_) => panic!("expected terminal mount spec"),
        }
    }

    #[test]
    fn browser_host_events_update_surface_metadata() {
        let core = SharedCore::bootstrap(bootstrap());
        core.split_with_browser();

        let browser = core
            .snapshot()
            .portal
            .panes
            .into_iter()
            .find(|pane| matches!(pane.mount, SurfaceMountSpec::Browser(_)))
            .expect("browser pane");

        core.apply_host_event(HostEvent::SurfaceTitleChanged {
            surface_id: browser.surface_id,
            title: "Taskers Docs".into(),
        });
        core.apply_host_event(HostEvent::SurfaceUrlChanged {
            surface_id: browser.surface_id,
            url: "https://taskers.invalid/docs".into(),
        });

        let snapshot = core.snapshot();
        let pane = match snapshot.layout {
            super::LayoutNodeSnapshot::Split { second, .. } => second,
            _ => panic!("expected split layout"),
        };
        let pane = match *pane {
            super::LayoutNodeSnapshot::Pane(pane) => pane,
            _ => panic!("expected pane node"),
        };
        assert_eq!(pane.surface.title, "Taskers Docs");
        assert_eq!(
            pane.surface.url.as_deref(),
            Some("https://taskers.invalid/docs")
        );
    }

    #[test]
    fn closing_a_split_surface_collapses_the_layout() {
        let core = SharedCore::bootstrap(bootstrap());
        core.split_with_browser();
        let browser = core
            .snapshot()
            .portal
            .panes
            .into_iter()
            .find(|pane| matches!(pane.mount, SurfaceMountSpec::Browser(BrowserMountSpec { .. })))
            .expect("browser pane");

        core.apply_host_event(HostEvent::SurfaceClosed {
            pane_id: browser.pane_id,
            surface_id: browser.surface_id,
        });

        let snapshot = core.snapshot();
        assert!(matches!(snapshot.layout, super::LayoutNodeSnapshot::Pane(_)));
        assert_eq!(snapshot.portal.panes.len(), 1);
    }

    #[test]
    fn runtime_status_round_trips_through_snapshot() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();

        assert!(matches!(
            snapshot.runtime_status.ghostty_runtime,
            RuntimeCapability::Ready
        ));
        assert!(matches!(
            snapshot.runtime_status.shell_integration,
            RuntimeCapability::Ready
        ));
        assert!(matches!(
            snapshot.runtime_status.terminal_host,
            RuntimeCapability::Fallback { .. }
        ));
    }
}
