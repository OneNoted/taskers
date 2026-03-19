use indexmap::IndexMap;
use parking_lot::Mutex;
use std::{collections::BTreeMap, fmt, sync::Arc};
use tokio::sync::{broadcast, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivityId(pub u64);

macro_rules! impl_display_id {
    ($name:ident, $prefix:literal) => {
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "-{}"), self.0)
            }
        }
    };
}

impl_display_id!(WorkspaceId, "workspace");
impl_display_id!(PaneId, "pane");
impl_display_id!(SurfaceId, "surface");
impl_display_id!(ActivityId, "activity");

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
pub enum AttentionState {
    Normal,
    Busy,
    Completed,
    WaitingInput,
    Error,
}

impl AttentionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Idle",
            Self::Busy => "Busy",
            Self::Completed => "Completed",
            Self::WaitingInput => "Waiting",
            Self::Error => "Error",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Busy => "busy",
            Self::Completed => "completed",
            Self::WaitingInput => "waiting",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSection {
    Workspace,
    Settings,
}

impl ShellSection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Workspace => "Workspace",
            Self::Settings => "Settings",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutPreset {
    Balanced,
    PowerUser,
}

impl ShortcutPreset {
    pub const ALL: [Self; 2] = [Self::Balanced, Self::PowerUser];

    pub fn id(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::PowerUser => "power-user",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced Defaults",
            Self::PowerUser => "Power User Defaults",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Balanced => {
                "Keep common focus, overview, browser, and split actions bound."
            }
            Self::PowerUser => {
                "Restore dense direction and resize bindings for full keyboard-driven control."
            }
        }
    }

    pub fn parse(id: &str) -> Option<Self> {
        match id {
            "balanced" => Some(Self::Balanced),
            "power-user" => Some(Self::PowerUser),
            _ => None,
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
    pub activity_width: i32,
    pub toolbar_height: i32,
    pub workspace_padding: i32,
    pub split_gap: i32,
    pub pane_header_height: i32,
    pub surface_tab_height: i32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            sidebar_width: 248,
            activity_width: 312,
            toolbar_height: 56,
            workspace_padding: 16,
            split_gap: 12,
            pane_header_height: 38,
            surface_tab_height: 34,
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
    pub attention: AttentionState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub active: bool,
    pub attention: AttentionState,
    pub active_surface: SurfaceId,
    pub surfaces: Vec<SurfaceSnapshot>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub title: String,
    pub preview: String,
    pub active: bool,
    pub pane_count: usize,
    pub surface_count: usize,
    pub unread_activity: usize,
    pub attention: AttentionState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceViewSnapshot {
    pub id: WorkspaceId,
    pub title: String,
    pub attention: AttentionState,
    pub pane_count: usize,
    pub surface_count: usize,
    pub active_pane: PaneId,
    pub layout: LayoutNodeSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityItemSnapshot {
    pub id: ActivityId,
    pub title: String,
    pub preview: String,
    pub meta: String,
    pub attention: AttentionState,
    pub workspace_id: WorkspaceId,
    pub pane_id: Option<PaneId>,
    pub surface_id: Option<SurfaceId>,
    pub unread: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeOptionSnapshot {
    pub id: String,
    pub label: String,
    pub family: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutPresetSnapshot {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutBindingSnapshot {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub category: String,
    pub accelerators: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub selected_theme_id: String,
    pub theme_options: Vec<ThemeOptionSnapshot>,
    pub shortcut_presets: Vec<ShortcutPresetSnapshot>,
    pub shortcuts: Vec<ShortcutBindingSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSnapshot {
    pub revision: u64,
    pub section: ShellSection,
    pub overview_mode: bool,
    pub workspaces: Vec<WorkspaceSummary>,
    pub current_workspace: WorkspaceViewSnapshot,
    pub activity: Vec<ActivityItemSnapshot>,
    pub portal: SurfacePortalPlan,
    pub metrics: LayoutMetrics,
    pub runtime_status: RuntimeStatus,
    pub settings: SettingsSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    PaneFocused { pane_id: PaneId },
    SurfaceClosed { pane_id: PaneId, surface_id: SurfaceId },
    SurfaceTitleChanged { surface_id: SurfaceId, title: String },
    SurfaceUrlChanged { surface_id: SurfaceId, url: String },
    SurfaceCwdChanged { surface_id: SurfaceId, cwd: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAction {
    ShowSection { section: ShellSection },
    ToggleOverview,
    FocusWorkspace { workspace_id: WorkspaceId },
    CreateWorkspace,
    SplitBrowser { pane_id: Option<PaneId> },
    SplitTerminal { pane_id: Option<PaneId> },
    AddBrowserSurface { pane_id: Option<PaneId> },
    AddTerminalSurface { pane_id: Option<PaneId> },
    FocusPane { pane_id: PaneId },
    FocusSurface { pane_id: PaneId, surface_id: SurfaceId },
    CloseSurface { pane_id: PaneId, surface_id: SurfaceId },
    DismissActivity { activity_id: ActivityId },
    SelectTheme { theme_id: String },
    SelectShortcutPreset { preset_id: String },
}

#[derive(Debug, Clone)]
struct SurfaceRecord {
    id: SurfaceId,
    kind: SurfaceKind,
    title: String,
    url: Option<String>,
    cwd: Option<String>,
    attention: AttentionState,
}

#[derive(Debug, Clone)]
struct PaneRecord {
    id: PaneId,
    active_surface: SurfaceId,
    surfaces: IndexMap<SurfaceId, SurfaceRecord>,
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
struct WorkspaceRecord {
    id: WorkspaceId,
    title: String,
    active_pane: PaneId,
    panes: IndexMap<PaneId, PaneRecord>,
    layout: LayoutNode,
}

#[derive(Debug, Clone)]
struct ActivityRecord {
    id: ActivityId,
    title: String,
    preview: String,
    meta: String,
    attention: AttentionState,
    workspace_id: WorkspaceId,
    pane_id: Option<PaneId>,
    surface_id: Option<SurfaceId>,
    unread: bool,
}

#[derive(Debug, Clone)]
struct AppModel {
    section: ShellSection,
    overview_mode: bool,
    active_workspace: WorkspaceId,
    workspaces: IndexMap<WorkspaceId, WorkspaceRecord>,
    activity: IndexMap<ActivityId, ActivityRecord>,
    selected_theme_id: String,
    selected_shortcut_preset: ShortcutPreset,
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
                section: ShellSection::Workspace,
                overview_mode: false,
                active_workspace: WorkspaceId(0),
                workspaces: IndexMap::new(),
                activity: IndexMap::new(),
                selected_theme_id: "dark".into(),
                selected_shortcut_preset: ShortcutPreset::Balanced,
                window_size: PixelSize::new(1440, 900),
            },
            runtime_status: bootstrap.runtime_status,
            terminal_defaults: bootstrap.terminal_defaults,
        };

        let main = core.seed_workspace(
            "Main",
            vec![
                SeedSurface::terminal("Agent shell", None, AttentionState::Busy).with_secondary(
                    SeedSurface::browser(
                        "Docs",
                        "https://taskers.app/docs",
                        AttentionState::Completed,
                    ),
                ),
                SeedPane::new(SeedSurface::browser(
                    "Preview",
                    "https://dioxuslabs.com/learn/0.7/",
                    AttentionState::Normal,
                )),
            ],
        );
        let research = core.seed_workspace(
            "Research",
            vec![SeedPane::new(SeedSurface::browser(
                "Taskers docs",
                "https://taskers.invalid/docs",
                AttentionState::WaitingInput,
            ))],
        );
        let release = core.seed_workspace(
            "Release",
            vec![SeedPane::new(SeedSurface::terminal(
                "Release checks",
                Some("/home/notes/Projects/taskers"),
                AttentionState::Error,
            ))],
        );

        core.model.active_workspace = main;
        core.seed_activity(
            "Embedded terminal host",
            match &core.runtime_status.terminal_host {
                RuntimeCapability::Ready => "Embedded Ghostty passed the last startup probe.".into(),
                RuntimeCapability::Fallback { message } => {
                    format!("Shell is still live, but terminal startup fell back: {message}")
                }
                RuntimeCapability::Unavailable { message } => {
                    format!("Embedded terminal host is unavailable: {message}")
                }
            },
            "Linux host runtime",
            match core.runtime_status.terminal_host {
                RuntimeCapability::Ready => AttentionState::Completed,
                RuntimeCapability::Fallback { .. } => AttentionState::WaitingInput,
                RuntimeCapability::Unavailable { .. } => AttentionState::Error,
            },
            main,
            None,
            None,
        );
        core.seed_activity(
            "Research workspace waiting",
            "A browser review is staged in the Research workspace.",
            "Workspace Research · browser",
            AttentionState::WaitingInput,
            research,
            None,
            None,
        );
        core.seed_activity(
            "Release checks need attention",
            "The release terminal recorded a failing verification run.",
            "Workspace Release · terminal",
            AttentionState::Error,
            release,
            None,
            None,
        );

        core
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn current_workspace(&self) -> &WorkspaceRecord {
        self.model
            .workspaces
            .get(&self.model.active_workspace)
            .expect("active workspace should exist")
    }

    fn snapshot(&self) -> ShellSnapshot {
        let workspace = self.current_workspace();
        let content = self.content_frame();
        let portal = SurfacePortalPlan {
            window: Frame::new(
                0,
                0,
                self.model.window_size.width,
                self.model.window_size.height,
            ),
            content,
            panes: if matches!(self.model.section, ShellSection::Workspace) {
                self.collect_surface_plans(workspace, &workspace.layout, content)
            } else {
                Vec::new()
            },
        };

        ShellSnapshot {
            revision: self.revision,
            section: self.model.section,
            overview_mode: self.model.overview_mode,
            workspaces: self.workspace_summaries(),
            current_workspace: WorkspaceViewSnapshot {
                id: workspace.id,
                title: workspace.title.clone(),
                attention: self.workspace_attention(workspace.id),
                pane_count: workspace.panes.len(),
                surface_count: workspace
                    .panes
                    .values()
                    .map(|pane| pane.surfaces.len())
                    .sum(),
                active_pane: workspace.active_pane,
                layout: self.snapshot_layout(workspace, &workspace.layout),
            },
            activity: self.activity_snapshot(),
            portal,
            metrics: self.metrics,
            runtime_status: self.runtime_status.clone(),
            settings: self.settings_snapshot(),
        }
    }

    fn workspace_summaries(&self) -> Vec<WorkspaceSummary> {
        self.model
            .workspaces
            .values()
            .map(|workspace| WorkspaceSummary {
                id: workspace.id,
                title: workspace.title.clone(),
                preview: self.workspace_preview(workspace),
                active: workspace.id == self.model.active_workspace,
                pane_count: workspace.panes.len(),
                surface_count: workspace
                    .panes
                    .values()
                    .map(|pane| pane.surfaces.len())
                    .sum(),
                unread_activity: self
                    .model
                    .activity
                    .values()
                    .filter(|item| item.workspace_id == workspace.id && item.unread)
                    .count(),
                attention: self.workspace_attention(workspace.id),
            })
            .collect()
    }

    fn workspace_preview(&self, workspace: &WorkspaceRecord) -> String {
        let pane = workspace
            .panes
            .get(&workspace.active_pane)
            .or_else(|| workspace.panes.values().next());
        let Some(pane) = pane else {
            return "No surfaces".into();
        };
        let surface = pane
            .surfaces
            .get(&pane.active_surface)
            .or_else(|| pane.surfaces.values().next())
            .expect("pane should have at least one surface");
        match surface.kind {
            SurfaceKind::Terminal => surface
                .cwd
                .clone()
                .unwrap_or_else(|| "Embedded terminal".into()),
            SurfaceKind::Browser => surface
                .url
                .clone()
                .unwrap_or_else(|| "Native browser view".into()),
        }
    }

    fn workspace_attention(&self, workspace_id: WorkspaceId) -> AttentionState {
        let pane_attention = self
            .model
            .workspaces
            .get(&workspace_id)
            .map(|workspace| {
                workspace
                    .panes
                    .values()
                    .flat_map(|pane| pane.surfaces.values().map(|surface| surface.attention))
                    .fold(AttentionState::Normal, strongest_attention)
            })
            .unwrap_or(AttentionState::Normal);

        self.model
            .activity
            .values()
            .filter(|item| item.workspace_id == workspace_id && item.unread)
            .map(|item| item.attention)
            .fold(pane_attention, strongest_attention)
    }

    fn activity_snapshot(&self) -> Vec<ActivityItemSnapshot> {
        self.model
            .activity
            .values()
            .rev()
            .map(|item| ActivityItemSnapshot {
                id: item.id,
                title: item.title.clone(),
                preview: item.preview.clone(),
                meta: item.meta.clone(),
                attention: item.attention,
                workspace_id: item.workspace_id,
                pane_id: item.pane_id,
                surface_id: item.surface_id,
                unread: item.unread,
            })
            .collect()
    }

    fn settings_snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            selected_theme_id: self.model.selected_theme_id.clone(),
            theme_options: builtin_theme_options(&self.model.selected_theme_id),
            shortcut_presets: ShortcutPreset::ALL
                .into_iter()
                .map(|preset| ShortcutPresetSnapshot {
                    id: preset.id().into(),
                    label: preset.label().into(),
                    detail: preset.detail().into(),
                    active: preset == self.model.selected_shortcut_preset,
                })
                .collect(),
            shortcuts: shortcut_bindings(self.model.selected_shortcut_preset),
        }
    }

    fn snapshot_layout(
        &self,
        workspace: &WorkspaceRecord,
        node: &LayoutNode,
    ) -> LayoutNodeSnapshot {
        match node {
            LayoutNode::Leaf(pane_id) => {
                let pane = workspace
                    .panes
                    .get(pane_id)
                    .expect("layout pane should exist");
                let surfaces = pane
                    .surfaces
                    .values()
                    .map(|surface| SurfaceSnapshot {
                        id: surface.id,
                        kind: surface.kind,
                        title: surface.title.clone(),
                        url: surface.url.clone(),
                        cwd: surface.cwd.clone(),
                        attention: surface.attention,
                    })
                    .collect::<Vec<_>>();

                LayoutNodeSnapshot::Pane(PaneSnapshot {
                    id: pane.id,
                    active: pane.id == workspace.active_pane,
                    attention: pane_attention(pane),
                    active_surface: pane.active_surface,
                    surfaces,
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
                first: Box::new(self.snapshot_layout(workspace, first)),
                second: Box::new(self.snapshot_layout(workspace, second)),
            },
        }
    }

    fn content_frame(&self) -> Frame {
        let metrics = self.metrics;
        let padding = metrics.workspace_padding;
        let x = metrics.sidebar_width + padding;
        let y = metrics.toolbar_height + padding;
        let width = (self.model.window_size.width
            - metrics.sidebar_width
            - metrics.activity_width
            - (padding * 2))
            .max(360);
        let height =
            (self.model.window_size.height - metrics.toolbar_height - (padding * 2)).max(240);
        Frame::new(x, y, width, height)
    }

    fn collect_surface_plans(
        &self,
        workspace: &WorkspaceRecord,
        node: &LayoutNode,
        frame: Frame,
    ) -> Vec<PortalSurfacePlan> {
        let mut panes = Vec::new();
        self.collect_surface_plans_into(workspace, node, frame, &mut panes);
        panes
    }

    fn collect_surface_plans_into(
        &self,
        workspace: &WorkspaceRecord,
        node: &LayoutNode,
        frame: Frame,
        out: &mut Vec<PortalSurfacePlan>,
    ) {
        match node {
            LayoutNode::Leaf(pane_id) => {
                if let Some(pane) = workspace.panes.get(pane_id) {
                    if let Some(surface) = pane
                        .surfaces
                        .get(&pane.active_surface)
                        .or_else(|| pane.surfaces.values().next())
                    {
                        out.push(PortalSurfacePlan {
                            pane_id: pane.id,
                            surface_id: surface.id,
                            active: pane.id == workspace.active_pane,
                            frame: frame
                                .inset_top(self.metrics.pane_header_height + self.metrics.surface_tab_height),
                            mount: self.mount_spec_for(workspace.id, pane.id, surface),
                        });
                    }
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
                self.collect_surface_plans_into(workspace, first, first_frame, out);
                self.collect_surface_plans_into(workspace, second, second_frame, out);
            }
        }
    }

    fn mount_spec_for(
        &self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface: &SurfaceRecord,
    ) -> SurfaceMountSpec {
        match surface.kind {
            SurfaceKind::Browser => SurfaceMountSpec::Browser(BrowserMountSpec {
                url: surface
                    .url
                    .clone()
                    .unwrap_or_else(|| "https://dioxuslabs.com/learn/0.7/".into()),
            }),
            SurfaceKind::Terminal => {
                let mut env = self.terminal_defaults.env.clone();
                env.insert("TASKERS_PANE_ID".into(), pane_id.to_string());
                env.insert("TASKERS_SURFACE_ID".into(), surface.id.to_string());
                env.insert("TASKERS_WORKSPACE_ID".into(), workspace_id.to_string());
                SurfaceMountSpec::Terminal(TerminalMountSpec {
                    title: surface.title.clone(),
                    cwd: surface.cwd.clone(),
                    cols: self.terminal_defaults.cols,
                    rows: self.terminal_defaults.rows,
                    command_argv: self.terminal_defaults.command_argv.clone(),
                    env,
                })
            }
        }
    }

    fn split_pane(&mut self, target: PaneId, kind: SurfaceKind, axis: SplitAxis) -> bool {
        let workspace_id = self.model.active_workspace;
        let new_pane = self.make_pane(
            kind,
            default_surface_title(kind),
            default_surface_url(kind),
            None,
            initial_attention_for(kind),
        );
        let pane_id = new_pane.id;
        let Some(workspace) = self.model.workspaces.get_mut(&workspace_id) else {
            return false;
        };
        if !workspace.panes.contains_key(&target) {
            return false;
        }
        workspace.panes.insert(pane_id, new_pane);
        if workspace.layout.split_leaf(target, axis, pane_id, 500) {
            workspace.active_pane = pane_id;
            self.revision += 1;
            true
        } else {
            let _ = workspace.panes.shift_remove(&pane_id);
            false
        }
    }

    fn add_surface_to_pane(&mut self, target: PaneId, kind: SurfaceKind) -> bool {
        let workspace_id = self.model.active_workspace;
        let surface = self.make_surface_record(
            kind,
            default_surface_title(kind),
            default_surface_url(kind),
            None,
            initial_attention_for(kind),
        );
        let surface_id = surface.id;
        let Some(workspace) = self.model.workspaces.get_mut(&workspace_id) else {
            return false;
        };
        let Some(pane) = workspace.panes.get_mut(&target) else {
            return false;
        };
        pane.surfaces.insert(surface_id, surface);
        pane.active_surface = surface_id;
        workspace.active_pane = target;
        self.revision += 1;
        true
    }

    fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        let workspace_id = self.model.active_workspace;
        let active_surface_id = {
            let Some(workspace) = self.model.workspaces.get_mut(&workspace_id) else {
                return false;
            };
            if !workspace.panes.contains_key(&pane_id) {
                return false;
            }
            workspace.active_pane = pane_id;
            if let Some(pane) = workspace.panes.get_mut(&pane_id) {
                if let Some(surface) = pane.surfaces.get_mut(&pane.active_surface) {
                    surface.attention = AttentionState::Normal;
                }
                Some(pane.active_surface)
            } else {
                None
            }
        };
        self.dismiss_surface_activity(workspace_id, Some(pane_id), active_surface_id);
        self.revision += 1;
        true
    }

    fn focus_surface(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let workspace_id = self.model.active_workspace;
        {
            let Some(workspace) = self.model.workspaces.get_mut(&workspace_id) else {
                return false;
            };
            let Some(pane) = workspace.panes.get_mut(&pane_id) else {
                return false;
            };
            if !pane.surfaces.contains_key(&surface_id) {
                return false;
            }
            pane.active_surface = surface_id;
            workspace.active_pane = pane_id;
            if let Some(surface) = pane.surfaces.get_mut(&surface_id) {
                surface.attention = AttentionState::Normal;
            }
        }
        self.dismiss_surface_activity(workspace_id, Some(pane_id), Some(surface_id));
        self.revision += 1;
        true
    }

    fn focus_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        if self.model.workspaces.contains_key(&workspace_id)
            && self.model.active_workspace != workspace_id
        {
            self.model.active_workspace = workspace_id;
            self.model.section = ShellSection::Workspace;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn create_workspace(&mut self) -> bool {
        let title = format!("Workspace {}", self.model.workspaces.len() + 1);
        let workspace_id = self.seed_workspace(
            &title,
            vec![SeedPane::new(SeedSurface::terminal(
                "Agent shell",
                None,
                initial_attention_for(SurfaceKind::Terminal),
            ))],
        );
        self.model.active_workspace = workspace_id;
        self.model.section = ShellSection::Workspace;
        self.revision += 1;
        true
    }

    fn set_section(&mut self, section: ShellSection) -> bool {
        if self.model.section != section {
            self.model.section = section;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn toggle_overview(&mut self) -> bool {
        self.model.overview_mode = !self.model.overview_mode;
        self.revision += 1;
        true
    }

    fn dismiss_activity(&mut self, activity_id: ActivityId) -> bool {
        if self.model.activity.shift_remove(&activity_id).is_some() {
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn select_theme(&mut self, theme_id: String) -> bool {
        if builtin_theme_options("")
            .iter()
            .any(|option| option.id == theme_id)
            && self.model.selected_theme_id != theme_id
        {
            self.model.selected_theme_id = theme_id;
            self.revision += 1;
            true
        } else {
            false
        }
    }

    fn select_shortcut_preset(&mut self, preset_id: String) -> bool {
        let Some(preset) = ShortcutPreset::parse(&preset_id) else {
            return false;
        };
        if self.model.selected_shortcut_preset != preset {
            self.model.selected_shortcut_preset = preset;
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
                        surface.attention = AttentionState::Completed;
                        true
                    } else {
                        false
                    }
                });
                self.push_surface_activity(
                    surface_id,
                    "Surface title updated",
                    title,
                    AttentionState::Completed,
                )
            }
            HostEvent::SurfaceUrlChanged { surface_id, url } => {
                self.update_surface(surface_id, |surface| {
                    if surface.url.as_deref() != Some(url.as_str()) {
                        surface.url = Some(url.clone());
                        surface.attention = AttentionState::Busy;
                        true
                    } else {
                        false
                    }
                });
                self.push_surface_activity(
                    surface_id,
                    "Browser navigated",
                    url,
                    AttentionState::Busy,
                )
            }
            HostEvent::SurfaceCwdChanged { surface_id, cwd } => {
                self.update_surface(surface_id, |surface| {
                    if surface.cwd.as_deref() != Some(cwd.as_str()) {
                        surface.cwd = Some(cwd.clone());
                        surface.attention = AttentionState::Busy;
                        true
                    } else {
                        false
                    }
                });
                self.push_surface_activity(
                    surface_id,
                    "Terminal changed directory",
                    cwd,
                    AttentionState::Busy,
                )
            }
        }
    }

    fn dispatch_shell_action(&mut self, action: ShellAction) -> bool {
        match action {
            ShellAction::ShowSection { section } => self.set_section(section),
            ShellAction::ToggleOverview => self.toggle_overview(),
            ShellAction::FocusWorkspace { workspace_id } => self.focus_workspace(workspace_id),
            ShellAction::CreateWorkspace => self.create_workspace(),
            ShellAction::SplitBrowser { pane_id } => self.split_pane(
                pane_id.unwrap_or(self.current_workspace().active_pane),
                SurfaceKind::Browser,
                SplitAxis::Horizontal,
            ),
            ShellAction::SplitTerminal { pane_id } => self.split_pane(
                pane_id.unwrap_or(self.current_workspace().active_pane),
                SurfaceKind::Terminal,
                SplitAxis::Vertical,
            ),
            ShellAction::AddBrowserSurface { pane_id } => self.add_surface_to_pane(
                pane_id.unwrap_or(self.current_workspace().active_pane),
                SurfaceKind::Browser,
            ),
            ShellAction::AddTerminalSurface { pane_id } => self.add_surface_to_pane(
                pane_id.unwrap_or(self.current_workspace().active_pane),
                SurfaceKind::Terminal,
            ),
            ShellAction::FocusPane { pane_id } => self.focus_pane(pane_id),
            ShellAction::FocusSurface {
                pane_id,
                surface_id,
            } => self.focus_surface(pane_id, surface_id),
            ShellAction::CloseSurface {
                pane_id,
                surface_id,
            } => self.close_surface(pane_id, surface_id),
            ShellAction::DismissActivity { activity_id } => self.dismiss_activity(activity_id),
            ShellAction::SelectTheme { theme_id } => self.select_theme(theme_id),
            ShellAction::SelectShortcutPreset { preset_id } => {
                self.select_shortcut_preset(preset_id)
            }
        }
    }

    fn close_surface(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let workspace_id = self.model.active_workspace;
        let mut needs_replacement = false;
        {
            let Some(workspace) = self.model.workspaces.get_mut(&workspace_id) else {
                return false;
            };
            let Some(pane) = workspace.panes.get(&pane_id) else {
                return false;
            };
            if !pane.surfaces.contains_key(&surface_id) {
                return false;
            }

            if pane.surfaces.len() > 1 {
                let pane = workspace
                    .panes
                    .get_mut(&pane_id)
                    .expect("pane should still exist");
                pane.surfaces.shift_remove(&surface_id);
                if pane.active_surface == surface_id {
                    pane.active_surface = *pane
                        .surfaces
                        .keys()
                        .next()
                        .expect("pane should still have a surface");
                }
                workspace.active_pane = pane_id;
            } else {
                workspace.panes.shift_remove(&pane_id);
                if let Some(layout) = workspace.layout.clone().remove_leaf(pane_id) {
                    workspace.layout = layout;
                } else {
                    needs_replacement = true;
                }
                if !needs_replacement && !workspace.panes.contains_key(&workspace.active_pane) {
                    workspace.active_pane = workspace.layout.first_leaf_id();
                }
            }
        }

        if needs_replacement {
            let replacement = self.make_pane(
                SurfaceKind::Terminal,
                "Agent shell".into(),
                None,
                None,
                initial_attention_for(SurfaceKind::Terminal),
            );
            let replacement_id = replacement.id;
            let workspace = self
                .model
                .workspaces
                .get_mut(&workspace_id)
                .expect("active workspace should exist");
            workspace.panes.insert(replacement_id, replacement);
            workspace.layout = LayoutNode::Leaf(replacement_id);
            workspace.active_pane = replacement_id;
        }

        self.dismiss_surface_activity(workspace_id, Some(pane_id), Some(surface_id));
        self.revision += 1;
        true
    }

    fn update_surface(
        &mut self,
        surface_id: SurfaceId,
        mut update: impl FnMut(&mut SurfaceRecord) -> bool,
    ) -> bool {
        for workspace in self.model.workspaces.values_mut() {
            for pane in workspace.panes.values_mut() {
                if let Some(surface) = pane.surfaces.get_mut(&surface_id) {
                    return update(surface);
                }
            }
        }
        false
    }

    fn push_surface_activity(
        &mut self,
        surface_id: SurfaceId,
        title: impl Into<String>,
        preview: impl Into<String>,
        attention: AttentionState,
    ) -> bool {
        let Some((workspace_id, pane_id, meta)) = self.lookup_surface_context(surface_id) else {
            return false;
        };
        self.model.activity.insert(
            ActivityId(self.next_id),
            ActivityRecord {
                id: ActivityId(self.next_id),
                title: title.into(),
                preview: preview.into(),
                meta,
                attention,
                workspace_id,
                pane_id: Some(pane_id),
                surface_id: Some(surface_id),
                unread: true,
            },
        );
        self.next_id += 1;
        self.revision += 1;
        true
    }

    fn lookup_surface_context(&self, surface_id: SurfaceId) -> Option<(WorkspaceId, PaneId, String)> {
        for workspace in self.model.workspaces.values() {
            for pane in workspace.panes.values() {
                if let Some(surface) = pane.surfaces.get(&surface_id) {
                    let meta = format!(
                        "{} · {}",
                        workspace.title,
                        match surface.kind {
                            SurfaceKind::Terminal => surface
                                .cwd
                                .clone()
                                .unwrap_or_else(|| surface.kind.label().into()),
                            SurfaceKind::Browser => surface
                                .url
                                .clone()
                                .unwrap_or_else(|| surface.kind.label().into()),
                        }
                    );
                    return Some((workspace.id, pane.id, meta));
                }
            }
        }
        None
    }

    fn dismiss_surface_activity(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: Option<PaneId>,
        surface_id: Option<SurfaceId>,
    ) {
        let remove = self
            .model
            .activity
            .iter()
            .filter_map(|(id, item)| {
                (item.workspace_id == workspace_id
                    && item.pane_id == pane_id
                    && item.surface_id == surface_id)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        for id in remove {
            self.model.activity.shift_remove(&id);
        }
    }

    fn seed_workspace(&mut self, title: &str, panes: Vec<SeedPane>) -> WorkspaceId {
        let workspace_id = WorkspaceId(self.next_id);
        self.next_id += 1;
        let mut pane_records = IndexMap::new();
        let mut layout: Option<LayoutNode> = None;
        let mut active_pane = None;

        for (index, seed) in panes.into_iter().enumerate() {
            let pane = self.make_seeded_pane(seed);
            let pane_id = pane.id;
            if index == 0 {
                active_pane = Some(pane_id);
                layout = Some(LayoutNode::Leaf(pane_id));
            } else if let Some(layout_node) = layout.as_mut() {
                let axis = if index % 2 == 0 {
                    SplitAxis::Vertical
                } else {
                    SplitAxis::Horizontal
                };
                let _ = layout_node.split_leaf(active_pane.expect("first pane"), axis, pane_id, 500);
            }
            pane_records.insert(pane_id, pane);
        }

        let active_pane = active_pane.expect("workspace should contain at least one pane");
        self.model.workspaces.insert(
            workspace_id,
            WorkspaceRecord {
                id: workspace_id,
                title: title.into(),
                active_pane,
                panes: pane_records,
                layout: layout.expect("workspace should contain at least one pane"),
            },
        );
        workspace_id
    }

    fn seed_activity(
        &mut self,
        title: impl Into<String>,
        preview: impl Into<String>,
        meta: impl Into<String>,
        attention: AttentionState,
        workspace_id: WorkspaceId,
        pane_id: Option<PaneId>,
        surface_id: Option<SurfaceId>,
    ) {
        let activity_id = ActivityId(self.next_id);
        self.next_id += 1;
        self.model.activity.insert(
            activity_id,
            ActivityRecord {
                id: activity_id,
                title: title.into(),
                preview: preview.into(),
                meta: meta.into(),
                attention,
                workspace_id,
                pane_id,
                surface_id,
                unread: true,
            },
        );
    }

    fn make_seeded_pane(&mut self, seed: SeedPane) -> PaneRecord {
        let pane_id = PaneId(self.next_id);
        self.next_id += 1;
        let mut surfaces = IndexMap::new();
        let mut active_surface = None;
        for (index, seed_surface) in seed.surfaces.into_iter().enumerate() {
            let surface = self.make_surface_record(
                seed_surface.kind,
                seed_surface.title,
                seed_surface.url,
                seed_surface.cwd,
                seed_surface.attention,
            );
            if index == 0 {
                active_surface = Some(surface.id);
            }
            surfaces.insert(surface.id, surface);
        }
        PaneRecord {
            id: pane_id,
            active_surface: active_surface.expect("seed pane should contain a surface"),
            surfaces,
        }
    }

    fn make_pane(
        &mut self,
        kind: SurfaceKind,
        title: String,
        url: Option<String>,
        cwd: Option<String>,
        attention: AttentionState,
    ) -> PaneRecord {
        let pane_id = PaneId(self.next_id);
        self.next_id += 1;
        let surface = self.make_surface_record(kind, title, url, cwd, attention);
        let active_surface = surface.id;
        let mut surfaces = IndexMap::new();
        surfaces.insert(surface.id, surface);
        PaneRecord {
            id: pane_id,
            active_surface,
            surfaces,
        }
    }

    fn make_surface_record(
        &mut self,
        kind: SurfaceKind,
        title: String,
        url: Option<String>,
        cwd: Option<String>,
        attention: AttentionState,
    ) -> SurfaceRecord {
        let surface_id = SurfaceId(self.next_id);
        self.next_id += 1;
        SurfaceRecord {
            id: surface_id,
            kind,
            title,
            url,
            cwd,
            attention,
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

fn strongest_attention(lhs: AttentionState, rhs: AttentionState) -> AttentionState {
    match (attention_rank(lhs), attention_rank(rhs)) {
        (left, right) if left >= right => lhs,
        _ => rhs,
    }
}

fn attention_rank(state: AttentionState) -> u8 {
    match state {
        AttentionState::Normal => 0,
        AttentionState::Completed => 1,
        AttentionState::Busy => 2,
        AttentionState::WaitingInput => 3,
        AttentionState::Error => 4,
    }
}

fn pane_attention(pane: &PaneRecord) -> AttentionState {
    pane.surfaces
        .values()
        .map(|surface| surface.attention)
        .fold(AttentionState::Normal, strongest_attention)
}

fn initial_attention_for(kind: SurfaceKind) -> AttentionState {
    match kind {
        SurfaceKind::Terminal => AttentionState::Busy,
        SurfaceKind::Browser => AttentionState::Completed,
    }
}

fn default_surface_title(kind: SurfaceKind) -> String {
    match kind {
        SurfaceKind::Terminal => "Agent shell".into(),
        SurfaceKind::Browser => "Browser".into(),
    }
}

fn default_surface_url(kind: SurfaceKind) -> Option<String> {
    match kind {
        SurfaceKind::Terminal => None,
        SurfaceKind::Browser => Some("https://dioxuslabs.com/learn/0.7/".into()),
    }
}

#[derive(Debug)]
struct SeedSurface {
    kind: SurfaceKind,
    title: String,
    url: Option<String>,
    cwd: Option<String>,
    attention: AttentionState,
}

impl SeedSurface {
    fn terminal(title: &str, cwd: Option<&str>, attention: AttentionState) -> Self {
        Self {
            kind: SurfaceKind::Terminal,
            title: title.into(),
            url: None,
            cwd: cwd.map(str::to_string),
            attention,
        }
    }

    fn browser(title: &str, url: &str, attention: AttentionState) -> Self {
        Self {
            kind: SurfaceKind::Browser,
            title: title.into(),
            url: Some(url.into()),
            cwd: None,
            attention,
        }
    }

    fn with_secondary(self, secondary: SeedSurface) -> SeedPane {
        SeedPane {
            surfaces: vec![self, secondary],
        }
    }
}

#[derive(Debug)]
struct SeedPane {
    surfaces: Vec<SeedSurface>,
}

impl SeedPane {
    fn new(surface: SeedSurface) -> Self {
        Self {
            surfaces: vec![surface],
        }
    }
}

const THEME_OPTIONS: &[(&str, &str, &str)] = &[
    ("dark", "Dark", "Default"),
    ("catppuccin-mocha", "Catppuccin Mocha", "Catppuccin"),
    ("tokyo-night", "Tokyo Night", "Tokyo Night"),
    ("gruvbox-dark", "Gruvbox Dark", "Other"),
];

fn builtin_theme_options(selected_theme_id: &str) -> Vec<ThemeOptionSnapshot> {
    THEME_OPTIONS
        .iter()
        .map(|(id, label, family)| ThemeOptionSnapshot {
            id: (*id).into(),
            label: (*label).into(),
            family: (*family).into(),
            active: *id == selected_theme_id,
        })
        .collect()
}

#[derive(Clone, Copy)]
struct ShortcutBindingSpec {
    id: &'static str,
    label: &'static str,
    detail: &'static str,
    category: &'static str,
    balanced: &'static [&'static str],
    power_user: &'static [&'static str],
}

const SHORTCUT_BINDINGS: &[ShortcutBindingSpec] = &[
    ShortcutBindingSpec { id: "toggle_overview", label: "Toggle overview", detail: "Zoom the current workspace out to fit the full column strip.", category: "General", balanced: &["<Control><Alt>o"], power_user: &["<Control><Alt>o"] },
    ShortcutBindingSpec { id: "close_terminal", label: "Close terminal", detail: "Close the active pane or active top-level window.", category: "General", balanced: &["<Control><Alt>x"], power_user: &["<Control><Alt>x"] },
    ShortcutBindingSpec { id: "open_browser_split", label: "Open browser in split", detail: "Split the active pane to the right and open a browser surface.", category: "Browser", balanced: &["<Control><Alt><Shift>l"], power_user: &["<Control><Alt><Shift>l"] },
    ShortcutBindingSpec { id: "focus_browser_address", label: "Focus browser address bar", detail: "Focus the address bar for the active browser surface.", category: "Browser", balanced: &["<Control>l"], power_user: &["<Control>l"] },
    ShortcutBindingSpec { id: "reload_browser_page", label: "Reload browser page", detail: "Reload the active browser surface.", category: "Browser", balanced: &["<Control>r"], power_user: &["<Control>r"] },
    ShortcutBindingSpec { id: "toggle_browser_devtools", label: "Toggle browser devtools", detail: "Show or hide devtools for the active browser surface.", category: "Browser", balanced: &["<Control><Shift>i"], power_user: &["<Control><Shift>i"] },
    ShortcutBindingSpec { id: "focus_left", label: "Focus left", detail: "Move focus to the column on the left, then fall back to pane focus.", category: "Focus", balanced: &["<Control><Alt>h", "<Control><Alt>Left"], power_user: &["<Control><Alt>h", "<Control><Alt>Left"] },
    ShortcutBindingSpec { id: "focus_right", label: "Focus right", detail: "Move focus to the column on the right, then fall back to pane focus.", category: "Focus", balanced: &["<Control><Alt>l", "<Control><Alt>Right"], power_user: &["<Control><Alt>l", "<Control><Alt>Right"] },
    ShortcutBindingSpec { id: "focus_up", label: "Focus up", detail: "Move focus to the stacked window above, then fall back to pane focus.", category: "Focus", balanced: &["<Control><Alt>k", "<Control><Alt>Up"], power_user: &["<Control><Alt>k", "<Control><Alt>Up"] },
    ShortcutBindingSpec { id: "focus_down", label: "Focus down", detail: "Move focus to the stacked window below, then fall back to pane focus.", category: "Focus", balanced: &["<Control><Alt>j", "<Control><Alt>Down"], power_user: &["<Control><Alt>j", "<Control><Alt>Down"] },
    ShortcutBindingSpec { id: "new_window_left", label: "New window left", detail: "Create a top-level window in a new column on the left.", category: "Top-level windows", balanced: &[], power_user: &["<Control><Alt><Shift>h", "<Control><Alt><Shift>Left"] },
    ShortcutBindingSpec { id: "new_window_right", label: "New window right", detail: "Create a top-level window in a new column on the right.", category: "Top-level windows", balanced: &["<Control><Alt>t"], power_user: &["<Control><Alt>t"] },
    ShortcutBindingSpec { id: "new_window_up", label: "New window up", detail: "Create a stacked top-level window above the active window.", category: "Top-level windows", balanced: &[], power_user: &["<Control><Alt><Shift>k", "<Control><Alt><Shift>Up"] },
    ShortcutBindingSpec { id: "new_window_down", label: "New window down", detail: "Create a stacked top-level window below the active window.", category: "Top-level windows", balanced: &["<Control><Alt>g"], power_user: &["<Control><Alt>g"] },
    ShortcutBindingSpec { id: "resize_window_left", label: "Make window narrower", detail: "Reduce the active column width.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt>Home"] },
    ShortcutBindingSpec { id: "resize_window_right", label: "Make window wider", detail: "Increase the active column width.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt>End"] },
    ShortcutBindingSpec { id: "resize_window_up", label: "Make window shorter", detail: "Reduce the active top-level window height.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt>Page_Up"] },
    ShortcutBindingSpec { id: "resize_window_down", label: "Make window taller", detail: "Increase the active top-level window height.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt>Page_Down"] },
    ShortcutBindingSpec { id: "resize_split_left", label: "Make split narrower", detail: "Reduce the active split width.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt><Shift>Home"] },
    ShortcutBindingSpec { id: "resize_split_right", label: "Make split wider", detail: "Increase the active split width.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt><Shift>End"] },
    ShortcutBindingSpec { id: "resize_split_up", label: "Make split shorter", detail: "Reduce the active split height.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt><Shift>Page_Up"] },
    ShortcutBindingSpec { id: "resize_split_down", label: "Make split taller", detail: "Increase the active split height.", category: "Advanced resize", balanced: &[], power_user: &["<Control><Alt><Shift>Page_Down"] },
    ShortcutBindingSpec { id: "split_right", label: "Split right", detail: "Split the active pane to the right inside the current window.", category: "Pane splits", balanced: &["<Control><Alt><Shift>t"], power_user: &["<Control><Alt><Shift>t"] },
    ShortcutBindingSpec { id: "split_down", label: "Split down", detail: "Split the active pane downward inside the current window.", category: "Pane splits", balanced: &["<Control><Alt><Shift>g"], power_user: &["<Control><Alt><Shift>g"] },
];

fn shortcut_bindings(preset: ShortcutPreset) -> Vec<ShortcutBindingSnapshot> {
    SHORTCUT_BINDINGS
        .iter()
        .map(|binding| ShortcutBindingSnapshot {
            id: binding.id.into(),
            label: binding.label.into(),
            detail: binding.detail.into(),
            category: binding.category.into(),
            accelerators: match preset {
                ShortcutPreset::Balanced => binding.balanced,
                ShortcutPreset::PowerUser => binding.power_user,
            }
            .iter()
            .map(|value| (*value).into())
            .collect(),
        })
        .collect()
}

#[derive(Clone)]
pub struct SharedCore {
    inner: Arc<Mutex<TaskersCore>>,
    revisions: watch::Sender<u64>,
    revision_events: broadcast::Sender<u64>,
}

impl SharedCore {
    pub fn bootstrap(bootstrap: BootstrapModel) -> Self {
        let core = TaskersCore::with_bootstrap(bootstrap);
        let (revisions, _) = watch::channel(core.revision());
        let (revision_events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Mutex::new(core)),
            revisions,
            revision_events,
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

    pub fn subscribe_revision_events(&self) -> broadcast::Receiver<u64> {
        self.revision_events.subscribe()
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.inner.lock().snapshot()
    }

    pub fn set_window_size(&self, size: PixelSize) {
        self.mutate(|core| core.set_window_size(size));
    }

    pub fn dispatch_shell_action(&self, action: ShellAction) {
        self.mutate(|core| core.dispatch_shell_action(action));
    }

    pub fn split_with_browser(&self) {
        self.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });
    }

    pub fn split_with_terminal(&self) {
        self.dispatch_shell_action(ShellAction::SplitTerminal { pane_id: None });
    }

    pub fn focus_pane(&self, pane_id: PaneId) {
        self.dispatch_shell_action(ShellAction::FocusPane { pane_id });
    }

    pub fn apply_host_event(&self, event: HostEvent) {
        self.mutate(|core| core.apply_host_event(event));
    }

    fn mutate(&self, update: impl FnOnce(&mut TaskersCore) -> bool) {
        let mut core = self.inner.lock();
        if update(&mut core) {
            let _ = self.revisions.send(core.revision());
            let _ = self.revision_events.send(core.revision());
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
        ShellAction, ShellSection, ShortcutPreset, SurfaceMountSpec, SurfaceKind,
        TerminalDefaults, WorkspaceId,
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
                    message: "GTK4 Ghostty bridge probe failed on this machine.".into(),
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
        let pane = match snapshot.current_workspace.layout {
            super::LayoutNodeSnapshot::Split { second, .. } => second,
            _ => panic!("expected split layout"),
        };
        let pane = match *pane {
            super::LayoutNodeSnapshot::Pane(pane) => pane,
            _ => panic!("expected pane node"),
        };
        let surface = pane
            .surfaces
            .into_iter()
            .find(|surface| surface.id == browser.surface_id)
            .expect("browser surface");
        assert_eq!(surface.title, "Taskers Docs");
        assert_eq!(
            surface.url.as_deref(),
            Some("https://taskers.invalid/docs")
        );
    }

    #[test]
    fn closing_a_split_surface_collapses_the_layout() {
        let core = SharedCore::bootstrap(bootstrap());
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
        assert!(matches!(
            snapshot.current_workspace.layout,
            super::LayoutNodeSnapshot::Pane(_)
        ));
        assert_eq!(snapshot.portal.panes.len(), 1);
    }

    #[test]
    fn adding_a_surface_switches_the_active_tab_and_portal_mount() {
        let core = SharedCore::bootstrap(bootstrap());
        let pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(pane_id),
        });

        let snapshot = core.snapshot();
        let pane = match snapshot.current_workspace.layout {
            super::LayoutNodeSnapshot::Split { first, .. } => first,
            super::LayoutNodeSnapshot::Pane(pane) => Box::new(super::LayoutNodeSnapshot::Pane(pane)),
        };
        let pane = match *pane {
            super::LayoutNodeSnapshot::Pane(pane) => pane,
            _ => panic!("expected pane"),
        };
        assert!(pane.surfaces.len() >= 3);
        let mounted = snapshot
            .portal
            .panes
            .into_iter()
            .find(|plan| plan.pane_id == pane_id)
            .expect("mounted plan for active pane");
        assert_eq!(mounted.surface_id, pane.active_surface);
        assert_eq!(mounted.mount.kind(), SurfaceKind::Browser);
    }

    #[test]
    fn switching_workspaces_updates_snapshot_and_portal() {
        let core = SharedCore::bootstrap(bootstrap());
        let workspace = core
            .snapshot()
            .workspaces
            .into_iter()
            .find(|workspace| workspace.title == "Research")
            .expect("research workspace");

        core.dispatch_shell_action(ShellAction::FocusWorkspace {
            workspace_id: workspace.id,
        });

        let snapshot = core.snapshot();
        assert_eq!(snapshot.current_workspace.id, workspace.id);
        assert_eq!(snapshot.current_workspace.title, "Research");
        assert_eq!(snapshot.portal.panes.len(), 1);
    }

    #[test]
    fn settings_snapshot_tracks_selected_theme_and_shortcut_preset() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Settings,
        });
        core.dispatch_shell_action(ShellAction::SelectTheme {
            theme_id: "tokyo-night".into(),
        });
        core.dispatch_shell_action(ShellAction::SelectShortcutPreset {
            preset_id: ShortcutPreset::PowerUser.id().into(),
        });

        let snapshot = core.snapshot();
        assert_eq!(snapshot.section, ShellSection::Settings);
        assert_eq!(snapshot.settings.selected_theme_id, "tokyo-night");
        assert!(
            snapshot
                .settings
                .shortcut_presets
                .iter()
                .any(|preset| preset.id == "power-user" && preset.active)
        );
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

    #[test]
    fn create_workspace_adds_new_sidebar_entry() {
        let core = SharedCore::bootstrap(bootstrap());
        let before = core.snapshot().workspaces.len();
        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let snapshot = core.snapshot();
        assert_eq!(snapshot.workspaces.len(), before + 1);
        assert!(snapshot
            .workspaces
            .iter()
            .any(|workspace| workspace.id == WorkspaceId(0) || workspace.active));
    }
}
