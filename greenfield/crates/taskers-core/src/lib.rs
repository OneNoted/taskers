use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    fmt,
    path::PathBuf,
    sync::Arc,
};
use taskers_app_core::{AppState, default_session_path};
use taskers_control::{ControlCommand, ControlResponse};
use taskers_domain::{
    ActivityItem, AppModel, PaneKind, PaneMetadata, PaneMetadataPatch,
    SplitAxis as DomainSplitAxis, SurfaceRecord, Workspace,
    WorkspaceSummary as DomainWorkspaceSummary,
};
use taskers_ghostty::{BackendChoice, SurfaceDescriptor};
use taskers_runtime::ShellLaunchSpec;
use tokio::sync::watch;

pub use taskers_domain::{PaneId, SurfaceId, WorkspaceId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivityId {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
}

impl fmt::Display for ActivityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "activity-{}-{}-{}",
            self.workspace_id, self.pane_id, self.surface_id
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

impl SplitAxis {
    fn from_domain(axis: DomainSplitAxis) -> Self {
        match axis {
            DomainSplitAxis::Horizontal => Self::Horizontal,
            DomainSplitAxis::Vertical => Self::Vertical,
        }
    }
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

    fn from_domain(kind: &PaneKind) -> Self {
        match kind {
            PaneKind::Terminal => Self::Terminal,
            PaneKind::Browser => Self::Browser,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

impl From<taskers_domain::AttentionState> for AttentionState {
    fn from(value: taskers_domain::AttentionState) -> Self {
        match value {
            taskers_domain::AttentionState::Normal => Self::Normal,
            taskers_domain::AttentionState::Busy => Self::Busy,
            taskers_domain::AttentionState::Completed => Self::Completed,
            taskers_domain::AttentionState::WaitingInput => Self::WaitingInput,
            taskers_domain::AttentionState::Error => Self::Error,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellSection {
    Workspace,
    Settings,
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
            Self::Balanced => "Apply Balanced Defaults",
            Self::PowerUser => "Apply Power User Defaults",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Balanced => {
                "Keep common focus, top-level window, split, overview, and close actions bound."
            }
            Self::PowerUser => {
                "Restore the dense direction and resize bindings for full keyboard-driven control."
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

#[derive(Clone)]
pub struct BootstrapModel {
    pub app_state: AppState,
    pub runtime_status: RuntimeStatus,
    pub selected_theme_id: String,
    pub selected_shortcut_preset: ShortcutPreset,
}

impl Default for BootstrapModel {
    fn default() -> Self {
        Self {
            app_state: default_preview_app_state(),
            runtime_status: RuntimeStatus::default(),
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: ShortcutPreset::Balanced,
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
struct UiState {
    section: ShellSection,
    overview_mode: bool,
    selected_theme_id: String,
    selected_shortcut_preset: ShortcutPreset,
    window_size: PixelSize,
}

#[derive(Clone)]
struct TaskersCore {
    app_state: AppState,
    revision: u64,
    metrics: LayoutMetrics,
    runtime_status: RuntimeStatus,
    ui: UiState,
}

impl TaskersCore {
    fn with_bootstrap(bootstrap: BootstrapModel) -> Self {
        let revision = bootstrap.app_state.revision().max(1);
        Self {
            app_state: bootstrap.app_state,
            revision,
            metrics: LayoutMetrics::default(),
            runtime_status: bootstrap.runtime_status,
            ui: UiState {
                section: ShellSection::Workspace,
                overview_mode: false,
                selected_theme_id: bootstrap.selected_theme_id,
                selected_shortcut_preset: bootstrap.selected_shortcut_preset,
                window_size: PixelSize::new(1440, 900),
            },
        }
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn snapshot(&self) -> ShellSnapshot {
        let model = self.app_state.snapshot_model();
        let workspace_id = model
            .active_workspace_id()
            .expect("active workspace should exist");
        let workspace = model
            .workspaces
            .get(&workspace_id)
            .expect("active workspace should exist");
        let active_window = workspace
            .active_window_record()
            .expect("active workspace window should exist");
        let content = self.content_frame();

        ShellSnapshot {
            revision: self.revision,
            section: self.ui.section,
            overview_mode: self.ui.overview_mode,
            workspaces: self.workspace_summaries(&model),
            current_workspace: WorkspaceViewSnapshot {
                id: workspace_id,
                title: workspace.label.clone(),
                attention: workspace_attention(workspace),
                pane_count: workspace.panes.len(),
                surface_count: workspace
                    .panes
                    .values()
                    .map(|pane| pane.surfaces.len())
                    .sum(),
                active_pane: workspace.active_pane,
                layout: self.snapshot_layout(workspace, &active_window.layout),
            },
            activity: self.activity_snapshot(&model),
            portal: SurfacePortalPlan {
                window: Frame::new(
                    0,
                    0,
                    self.ui.window_size.width,
                    self.ui.window_size.height,
                ),
                content,
                panes: if matches!(self.ui.section, ShellSection::Workspace) {
                    self.collect_surface_plans(workspace_id, workspace, &active_window.layout, content)
                } else {
                    Vec::new()
                },
            },
            metrics: self.metrics,
            runtime_status: self.runtime_status.clone(),
            settings: self.settings_snapshot(),
        }
    }

    fn content_frame(&self) -> Frame {
        let metrics = self.metrics;
        let width = (self.ui.window_size.width - metrics.sidebar_width - metrics.activity_width)
            .max(640);
        let height = self.ui.window_size.height.max(320);
        Frame::new(metrics.sidebar_width, 0, width, height)
    }

    fn settings_snapshot(&self) -> SettingsSnapshot {
        SettingsSnapshot {
            selected_theme_id: self.ui.selected_theme_id.clone(),
            theme_options: builtin_theme_options(&self.ui.selected_theme_id),
            shortcut_presets: ShortcutPreset::ALL
                .into_iter()
                .map(|preset| ShortcutPresetSnapshot {
                    id: preset.id().into(),
                    label: preset.label().into(),
                    detail: preset.detail().into(),
                    active: preset == self.ui.selected_shortcut_preset,
                })
                .collect(),
            shortcuts: shortcut_bindings(self.ui.selected_shortcut_preset),
        }
    }

    fn workspace_summaries(&self, model: &AppModel) -> Vec<WorkspaceSummary> {
        let active_window = model.active_window;
        model
            .workspace_summaries(active_window)
            .unwrap_or_default()
            .into_iter()
            .map(|summary| WorkspaceSummary {
                id: summary.workspace_id,
                title: summary.label.clone(),
                preview: workspace_preview(&summary),
                active: model.active_workspace_id() == Some(summary.workspace_id),
                pane_count: model
                    .workspaces
                    .get(&summary.workspace_id)
                    .map(|workspace| workspace.panes.len())
                    .unwrap_or_default(),
                surface_count: model
                    .workspaces
                    .get(&summary.workspace_id)
                    .map(workspace_surface_count)
                    .unwrap_or_default(),
                unread_activity: summary.unread_count,
                attention: summary.display_attention.into(),
            })
            .collect()
    }

    fn activity_snapshot(&self, model: &AppModel) -> Vec<ActivityItemSnapshot> {
        model.activity_items()
            .into_iter()
            .map(|item| ActivityItemSnapshot {
                id: ActivityId {
                    workspace_id: item.workspace_id,
                    pane_id: item.pane_id,
                    surface_id: item.surface_id,
                },
                title: activity_title(model, &item),
                preview: compact_preview(&item.message),
                meta: activity_context_line(model, &item),
                attention: item.state.into(),
                workspace_id: item.workspace_id,
                pane_id: Some(item.pane_id),
                surface_id: Some(item.surface_id),
                unread: true,
            })
            .collect()
    }

    fn snapshot_layout(
        &self,
        workspace: &Workspace,
        node: &taskers_domain::LayoutNode,
    ) -> LayoutNodeSnapshot {
        match node {
            taskers_domain::LayoutNode::Leaf { pane_id } => LayoutNodeSnapshot::Pane(
                self.pane_snapshot(
                    workspace,
                    workspace
                        .panes
                        .get(pane_id)
                        .expect("layout leaf should reference a pane"),
                ),
            ),
            taskers_domain::LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => LayoutNodeSnapshot::Split {
                axis: SplitAxis::from_domain(*axis),
                ratio: f32::from(*ratio) / 1000.0,
                first: Box::new(self.snapshot_layout(workspace, first)),
                second: Box::new(self.snapshot_layout(workspace, second)),
            },
        }
    }

    fn pane_snapshot(&self, workspace: &Workspace, pane: &taskers_domain::PaneRecord) -> PaneSnapshot {
        PaneSnapshot {
            id: pane.id,
            active: workspace.active_pane == pane.id,
            attention: pane.highest_attention().into(),
            active_surface: pane.active_surface,
            surfaces: pane
                .surfaces
                .values()
                .map(|surface| SurfaceSnapshot {
                    id: surface.id,
                    kind: SurfaceKind::from_domain(&surface.kind),
                    title: display_surface_title(surface),
                    url: normalized_surface_url(surface),
                    cwd: normalized_cwd(&surface.metadata),
                    attention: surface.attention.into(),
                })
                .collect(),
        }
    }

    fn collect_surface_plans(
        &self,
        workspace_id: WorkspaceId,
        workspace: &Workspace,
        node: &taskers_domain::LayoutNode,
        frame: Frame,
    ) -> Vec<PortalSurfacePlan> {
        match node {
            taskers_domain::LayoutNode::Leaf { pane_id } => workspace
                .panes
                .get(pane_id)
                .and_then(|pane| {
                    let active_surface = pane.active_surface()?;
                    Some(PortalSurfacePlan {
                        pane_id: pane.id,
                        surface_id: active_surface.id,
                        active: workspace.active_pane == pane.id,
                        frame: pane_body_frame(frame, self.metrics),
                        mount: self.mount_spec_for_active_surface(workspace_id, pane, active_surface),
                    })
                })
                .into_iter()
                .collect(),
            taskers_domain::LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let (first_frame, second_frame) =
                    split_frame(frame, SplitAxis::from_domain(*axis), *ratio, self.metrics.split_gap);
                let mut plans =
                    self.collect_surface_plans(workspace_id, workspace, first, first_frame);
                plans.extend(self.collect_surface_plans(
                    workspace_id,
                    workspace,
                    second,
                    second_frame,
                ));
                plans
            }
        }
    }

    fn mount_spec_for_active_surface(
        &self,
        workspace_id: WorkspaceId,
        pane: &taskers_domain::PaneRecord,
        active_surface: &SurfaceRecord,
    ) -> SurfaceMountSpec {
        let descriptor = self
            .app_state
            .surface_descriptor_for_pane(workspace_id, pane.id)
            .unwrap_or_else(|_| fallback_surface_descriptor(active_surface));
        mount_spec_from_descriptor(active_surface, descriptor)
    }

    fn set_window_size(&mut self, size: PixelSize) -> bool {
        if self.ui.window_size == size {
            return false;
        }
        self.ui.window_size = size;
        self.bump_local_revision();
        true
    }

    fn apply_host_event(&mut self, event: HostEvent) -> bool {
        match event {
            HostEvent::PaneFocused { pane_id } => self.focus_pane_by_id(pane_id),
            HostEvent::SurfaceClosed { pane_id, surface_id } => {
                self.close_surface_by_id(pane_id, surface_id)
            }
            HostEvent::SurfaceTitleChanged { surface_id, title } => self.update_surface_metadata(
                surface_id,
                PaneMetadataPatch {
                    title: Some(title),
                    ..PaneMetadataPatch::default()
                },
            ),
            HostEvent::SurfaceUrlChanged { surface_id, url } => self.update_surface_metadata(
                surface_id,
                PaneMetadataPatch {
                    url: Some(url),
                    ..PaneMetadataPatch::default()
                },
            ),
            HostEvent::SurfaceCwdChanged { surface_id, cwd } => self.update_surface_metadata(
                surface_id,
                PaneMetadataPatch {
                    cwd: Some(cwd),
                    ..PaneMetadataPatch::default()
                },
            ),
        }
    }

    fn dispatch_shell_action(&mut self, action: ShellAction) -> bool {
        match action {
            ShellAction::ShowSection { section } => {
                if self.ui.section == section {
                    return false;
                }
                self.ui.section = section;
                self.bump_local_revision();
                true
            }
            ShellAction::ToggleOverview => {
                self.ui.overview_mode = !self.ui.overview_mode;
                self.bump_local_revision();
                true
            }
            ShellAction::FocusWorkspace { workspace_id } => self.focus_workspace(workspace_id),
            ShellAction::CreateWorkspace => self.create_workspace(),
            ShellAction::SplitBrowser { pane_id } => self.split_with_kind(pane_id, PaneKind::Browser),
            ShellAction::SplitTerminal { pane_id } => self.split_with_kind(pane_id, PaneKind::Terminal),
            ShellAction::AddBrowserSurface { pane_id } => {
                self.add_surface_to_pane(pane_id, PaneKind::Browser)
            }
            ShellAction::AddTerminalSurface { pane_id } => {
                self.add_surface_to_pane(pane_id, PaneKind::Terminal)
            }
            ShellAction::FocusPane { pane_id } => self.focus_pane_by_id(pane_id),
            ShellAction::FocusSurface { pane_id, surface_id } => {
                self.focus_surface_by_id(pane_id, surface_id)
            }
            ShellAction::CloseSurface { pane_id, surface_id } => {
                self.close_surface_by_id(pane_id, surface_id)
            }
            ShellAction::DismissActivity { activity_id } => self.dismiss_activity(activity_id),
            ShellAction::SelectTheme { theme_id } => {
                if self.ui.selected_theme_id == theme_id {
                    return false;
                }
                self.ui.selected_theme_id = theme_id;
                self.bump_local_revision();
                true
            }
            ShellAction::SelectShortcutPreset { preset_id } => {
                let Some(preset) = ShortcutPreset::parse(&preset_id) else {
                    return false;
                };
                if self.ui.selected_shortcut_preset == preset {
                    return false;
                }
                self.ui.selected_shortcut_preset = preset;
                self.bump_local_revision();
                true
            }
        }
    }

    fn focus_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        let mut changed = false;
        if self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id) {
            changed |= self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        if self.ui.section != ShellSection::Workspace {
            self.ui.section = ShellSection::Workspace;
            self.bump_local_revision();
            changed = true;
        }
        changed
    }

    fn create_workspace(&mut self) -> bool {
        let label = next_workspace_label(&self.app_state.snapshot_model());
        self.dispatch_control(ControlCommand::CreateWorkspace { label })
    }

    fn split_with_kind(&mut self, pane_id: Option<PaneId>, kind: PaneKind) -> bool {
        let Some((workspace_id, target_pane_id)) = self.resolve_target_pane(pane_id) else {
            return false;
        };

        let response = match self.dispatch_control_with_response(ControlCommand::SplitPane {
            workspace_id,
            pane_id: Some(target_pane_id),
            axis: DomainSplitAxis::Horizontal,
        }) {
            Some(response) => response,
            None => return false,
        };

        let ControlResponse::PaneSplit { pane_id: new_pane_id } = response else {
            return false;
        };

        if kind == PaneKind::Terminal {
            return true;
        }

        let placeholder_surface = self
            .app_state
            .snapshot_model()
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&new_pane_id))
            .map(|pane| pane.active_surface);

        let created = self.dispatch_control(ControlCommand::CreateSurface {
            workspace_id,
            pane_id: new_pane_id,
            kind,
        });
        if !created {
            return false;
        }

        if let Some(placeholder_surface) = placeholder_surface {
            let _ = self.dispatch_control(ControlCommand::CloseSurface {
                workspace_id,
                pane_id: new_pane_id,
                surface_id: placeholder_surface,
            });
        }
        true
    }

    fn add_surface_to_pane(&mut self, pane_id: Option<PaneId>, kind: PaneKind) -> bool {
        let Some((workspace_id, target_pane_id)) = self.resolve_target_pane(pane_id) else {
            return false;
        };
        self.dispatch_control(ControlCommand::CreateSurface {
            workspace_id,
            pane_id: target_pane_id,
            kind,
        })
    }

    fn focus_pane_by_id(&mut self, pane_id: PaneId) -> bool {
        let Some((workspace_id, _)) = self.resolve_workspace_pane(&self.app_state.snapshot_model(), pane_id) else {
            return false;
        };
        if self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id) {
            let _ = self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        self.dispatch_control(ControlCommand::FocusPane { workspace_id, pane_id })
    }

    fn focus_surface_by_id(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let Some((workspace_id, _)) =
            self.resolve_surface_location(&self.app_state.snapshot_model(), surface_id)
        else {
            return false;
        };
        if self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id) {
            let _ = self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        self.dispatch_control(ControlCommand::FocusSurface {
            workspace_id,
            pane_id,
            surface_id,
        })
    }

    fn close_surface_by_id(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let Some((workspace_id, _)) =
            self.resolve_surface_location(&self.app_state.snapshot_model(), surface_id)
        else {
            return false;
        };
        self.dispatch_control(ControlCommand::CloseSurface {
            workspace_id,
            pane_id,
            surface_id,
        })
    }

    fn dismiss_activity(&mut self, activity_id: ActivityId) -> bool {
        self.dispatch_control(ControlCommand::MarkSurfaceCompleted {
            workspace_id: activity_id.workspace_id,
            pane_id: activity_id.pane_id,
            surface_id: activity_id.surface_id,
        })
    }

    fn update_surface_metadata(
        &mut self,
        surface_id: SurfaceId,
        patch: PaneMetadataPatch,
    ) -> bool {
        self.dispatch_control(ControlCommand::UpdateSurfaceMetadata { surface_id, patch })
    }

    fn resolve_target_pane(&self, pane_id: Option<PaneId>) -> Option<(WorkspaceId, PaneId)> {
        let model = self.app_state.snapshot_model();
        if let Some(pane_id) = pane_id {
            return self.resolve_workspace_pane(&model, pane_id);
        }
        let workspace_id = model.active_workspace_id()?;
        let workspace = model.workspaces.get(&workspace_id)?;
        Some((workspace_id, workspace.active_pane))
    }

    fn resolve_workspace_pane(
        &self,
        model: &AppModel,
        pane_id: PaneId,
    ) -> Option<(WorkspaceId, PaneId)> {
        model.workspaces.iter().find_map(|(workspace_id, workspace)| {
            workspace.panes.contains_key(&pane_id).then_some((*workspace_id, pane_id))
        })
    }

    fn resolve_surface_location(
        &self,
        model: &AppModel,
        surface_id: SurfaceId,
    ) -> Option<(WorkspaceId, PaneId)> {
        model.workspaces.iter().find_map(|(workspace_id, workspace)| {
            workspace.panes.iter().find_map(|(pane_id, pane)| {
                pane.surfaces
                    .contains_key(&surface_id)
                    .then_some((*workspace_id, *pane_id))
            })
        })
    }

    fn dispatch_control(&mut self, command: ControlCommand) -> bool {
        self.dispatch_control_with_response(command).is_some()
    }

    fn dispatch_control_with_response(
        &mut self,
        command: ControlCommand,
    ) -> Option<ControlResponse> {
        match self.app_state.dispatch(command) {
            Ok(response) => {
                self.sync_revision_from_app();
                Some(response)
            }
            Err(error) => {
                eprintln!("greenfield control dispatch failed: {error}");
                None
            }
        }
    }

    fn sync_revision_from_app(&mut self) {
        self.revision = self.revision.max(self.app_state.revision());
    }

    fn bump_local_revision(&mut self) {
        self.revision = self.revision.max(self.app_state.revision()) + 1;
    }
}

#[derive(Clone)]
pub struct SharedCore {
    inner: Arc<Mutex<TaskersCore>>,
    revisions: watch::Sender<u64>,
}

impl PartialEq for SharedCore {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SharedCore {}

impl SharedCore {
    pub fn bootstrap(bootstrap: BootstrapModel) -> Self {
        let core = TaskersCore::with_bootstrap(bootstrap);
        let revision = core.revision();
        let (revisions, _) = watch::channel(revision);
        Self {
            inner: Arc::new(Mutex::new(core)),
            revisions,
        }
    }

    pub fn subscribe_revisions(&self) -> watch::Receiver<u64> {
        self.revisions.subscribe()
    }

    pub fn revision(&self) -> u64 {
        self.inner.lock().revision()
    }

    pub fn snapshot(&self) -> ShellSnapshot {
        self.inner.lock().snapshot()
    }

    pub fn set_window_size(&self, size: PixelSize) {
        let mut inner = self.inner.lock();
        if inner.set_window_size(size) {
            let _ = self.revisions.send(inner.revision());
        }
    }

    pub fn dispatch_shell_action(&self, action: ShellAction) {
        let mut inner = self.inner.lock();
        if inner.dispatch_shell_action(action) {
            let _ = self.revisions.send(inner.revision());
        }
    }

    pub fn apply_host_event(&self, event: HostEvent) {
        let mut inner = self.inner.lock();
        if inner.apply_host_event(event) {
            let _ = self.revisions.send(inner.revision());
        }
    }

    pub fn split_with_browser(&self) {
        self.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });
    }

    pub fn split_with_terminal(&self) {
        self.dispatch_shell_action(ShellAction::SplitTerminal { pane_id: None });
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

fn default_preview_app_state() -> AppState {
    let mut model = AppModel::new("Main");
    let workspace_id = model.active_workspace_id().expect("workspace");
    let pane_id = model.active_workspace().expect("workspace").active_pane;
    let browser_pane_id = model
        .split_pane(workspace_id, Some(pane_id), DomainSplitAxis::Horizontal)
        .expect("split pane");
    let placeholder_surface_id = model
        .workspaces
        .get(&workspace_id)
        .and_then(|workspace| workspace.panes.get(&browser_pane_id))
        .map(|pane| pane.active_surface);
    let browser_surface_id = model
        .create_surface(workspace_id, browser_pane_id, PaneKind::Browser)
        .expect("browser surface");
    let _ = model.update_pane_metadata(
        pane_id,
        PaneMetadataPatch {
            title: Some("Agent shell".into()),
            cwd: std::env::current_dir()
                .ok()
                .map(|path| path.display().to_string()),
            ..PaneMetadataPatch::default()
        },
    );
    let _ = model.update_surface_metadata(
        browser_surface_id,
        PaneMetadataPatch {
            title: Some("Dioxus Tutorial".into()),
            url: Some("https://dioxuslabs.com/learn/0.7/tutorial/".into()),
            ..PaneMetadataPatch::default()
        },
    );
    if let Some(placeholder_surface_id) = placeholder_surface_id {
        let _ = model.close_surface(workspace_id, browser_pane_id, placeholder_surface_id);
    }

    AppState::new(
        model,
        default_session_path_for_preview("greenfield-preview-bootstrap"),
        BackendChoice::Mock,
        ShellLaunchSpec::fallback(),
    )
    .expect("preview app state")
}

fn default_session_path_for_preview(label: &str) -> PathBuf {
    let base = default_session_path();
    let stem = base
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("taskers-session");
    let file = format!("{stem}-{label}.json");
    base.with_file_name(file)
}

fn split_frame(frame: Frame, axis: SplitAxis, ratio: u16, gap: i32) -> (Frame, Frame) {
    let ratio = i32::from(ratio.clamp(150, 850));
    match axis {
        SplitAxis::Horizontal => {
            let usable_width = (frame.width - gap).max(2);
            let first_width = ((usable_width * ratio) / 1000).max(1);
            let second_width = (usable_width - first_width).max(1);
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
            let usable_height = (frame.height - gap).max(2);
            let first_height = ((usable_height * ratio) / 1000).max(1);
            let second_height = (usable_height - first_height).max(1);
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

fn pane_body_frame(frame: Frame, metrics: LayoutMetrics) -> Frame {
    frame.inset_top(metrics.pane_header_height + metrics.surface_tab_height)
}

fn workspace_preview(summary: &DomainWorkspaceSummary) -> String {
    if let Some(notification) = summary.latest_notification.as_deref() {
        return compact_preview(notification);
    }
    if let Some(repo_hint) = summary.repo_hint.as_deref() {
        return repo_hint.to_string();
    }
    if let Some(agent) = summary.agent_summaries.first() {
        return agent
            .title
            .clone()
            .unwrap_or_else(|| format!("{} {}", agent.agent_kind, agent.state.label()));
    }
    "No recent activity.".into()
}

fn workspace_surface_count(workspace: &Workspace) -> usize {
    workspace
        .panes
        .values()
        .map(|pane| pane.surfaces.len())
        .sum()
}

fn workspace_attention(workspace: &Workspace) -> AttentionState {
    workspace
        .panes
        .values()
        .map(|pane| pane.highest_attention())
        .max_by_key(|attention| attention.rank())
        .unwrap_or(taskers_domain::AttentionState::Normal)
        .into()
}

fn display_surface_title(surface: &SurfaceRecord) -> String {
    if let Some(title) = surface
        .metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return title.to_string();
    }

    if matches!(surface.kind, PaneKind::Browser)
        && let Some(url) = surface
            .metadata
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
    {
        return url.to_string();
    }

    match surface.kind {
        PaneKind::Terminal => "Terminal".into(),
        PaneKind::Browser => "Browser".into(),
    }
}

fn normalized_surface_url(surface: &SurfaceRecord) -> Option<String> {
    surface
        .metadata
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
}

fn normalized_cwd(metadata: &PaneMetadata) -> Option<String> {
    metadata
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(str::to_string)
}

fn compact_preview(message: &str) -> String {
    let trimmed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.len() <= 140 {
        return trimmed;
    }
    format!("{}…", &trimmed[..139])
}

fn activity_title(model: &AppModel, item: &ActivityItem) -> String {
    model.workspaces
        .get(&item.workspace_id)
        .and_then(|workspace| workspace.panes.get(&item.pane_id))
        .and_then(|pane| {
            pane.surfaces
                .get(&item.surface_id)
                .or_else(|| pane.active_surface())
        })
        .map(display_surface_title)
        .unwrap_or_else(|| "Terminal pane".into())
}

fn activity_context_line(model: &AppModel, item: &ActivityItem) -> String {
    let workspace_label = model
        .workspaces
        .get(&item.workspace_id)
        .map(|workspace| workspace.label.clone())
        .unwrap_or_else(|| "Workspace".into());
    let mut parts = vec![format!("Workspace {workspace_label}")];
    if let Some(surface) = model
        .workspaces
        .get(&item.workspace_id)
        .and_then(|workspace| workspace.panes.get(&item.pane_id))
        .and_then(|pane| {
            pane.surfaces
                .get(&item.surface_id)
                .or_else(|| pane.active_surface())
        })
    {
        parts.push(match surface.kind {
            PaneKind::Terminal => "terminal".into(),
            PaneKind::Browser => "browser".into(),
        });
        if let Some(repo) = surface.metadata.repo_name.as_deref() {
            parts.push(repo.to_string());
        }
        if let Some(branch) = surface.metadata.git_branch.as_deref() {
            parts.push(branch.to_string());
        }
    }
    parts.join(" · ")
}

fn next_workspace_label(model: &AppModel) -> String {
    format!("Workspace {}", model.workspaces.len() + 1)
}

fn fallback_surface_descriptor(surface: &SurfaceRecord) -> SurfaceDescriptor {
    SurfaceDescriptor {
        cols: 120,
        rows: 40,
        kind: surface.kind.clone(),
        cwd: normalized_cwd(&surface.metadata),
        title: surface
            .metadata
            .title
            .as_deref()
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(str::to_string),
        url: normalized_surface_url(surface),
        command_argv: Vec::new(),
        env: BTreeMap::new(),
    }
}

fn mount_spec_from_descriptor(
    surface: &SurfaceRecord,
    descriptor: SurfaceDescriptor,
) -> SurfaceMountSpec {
    match surface.kind {
        PaneKind::Browser => SurfaceMountSpec::Browser(BrowserMountSpec {
            url: descriptor
                .url
                .as_deref()
                .map(resolved_browser_uri)
                .unwrap_or_else(|| "about:blank".into()),
        }),
        PaneKind::Terminal => SurfaceMountSpec::Terminal(TerminalMountSpec {
            title: descriptor
                .title
                .unwrap_or_else(|| display_surface_title(surface)),
            cwd: descriptor.cwd,
            cols: descriptor.cols,
            rows: descriptor.rows,
            command_argv: descriptor.command_argv,
            env: descriptor.env,
        }),
    }
}

fn resolved_browser_uri(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "about:blank".into();
    }
    if trimmed.contains("://") {
        return trimmed.to_string();
    }
    if trimmed.chars().any(char::is_whitespace) {
        return format!(
            "https://duckduckgo.com/?q={}",
            trimmed.split_whitespace().collect::<Vec<_>>().join("+")
        );
    }
    if is_local_browser_target(trimmed) {
        return format!("http://{trimmed}");
    }
    format!("https://{trimmed}")
}

fn is_local_browser_target(value: &str) -> bool {
    value.starts_with("localhost")
        || value.starts_with("127.0.0.1")
        || value.starts_with("0.0.0.0")
        || value.contains(":3000")
        || value.contains(":5173")
        || value.contains(":8000")
        || value.contains(":8080")
}

#[cfg(test)]
mod tests {
    use super::{
        BootstrapModel, BrowserMountSpec, HostEvent, RuntimeCapability, RuntimeStatus, SharedCore,
        ShellAction, ShellSection, SurfaceMountSpec, default_preview_app_state,
    };

    fn bootstrap() -> BootstrapModel {
        BootstrapModel {
            app_state: default_preview_app_state(),
            runtime_status: RuntimeStatus {
                ghostty_runtime: RuntimeCapability::Ready,
                shell_integration: RuntimeCapability::Ready,
                terminal_host: RuntimeCapability::Fallback {
                    message: "Probe failed".into(),
                },
            },
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: super::ShortcutPreset::Balanced,
        }
    }

    #[test]
    fn default_bootstrap_projects_browser_and_terminal_portal_plans() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();

        let browser_count = snapshot
            .portal
            .panes
            .iter()
            .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Browser(_)))
            .count();
        let terminal_count = snapshot
            .portal
            .panes
            .iter()
            .filter(|plan| matches!(plan.mount, SurfaceMountSpec::Terminal(_)))
            .count();

        assert_eq!(browser_count, 1);
        assert_eq!(terminal_count, 1);
    }

    #[test]
    fn split_browser_creates_real_browser_pane() {
        let core = SharedCore::bootstrap(bootstrap());
        let before = core.snapshot().portal.panes.len();

        core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });

        let snapshot = core.snapshot();
        assert!(snapshot.portal.panes.len() > before);
        assert!(snapshot.portal.panes.iter().any(|plan| {
            matches!(
                &plan.mount,
                SurfaceMountSpec::Browser(BrowserMountSpec { url })
                    if url.starts_with("http")
            )
        }));
    }

    #[test]
    fn host_events_round_trip_surface_metadata_into_snapshot() {
        let core = SharedCore::bootstrap(bootstrap());
        let browser_surface = core
            .snapshot()
            .portal
            .panes
            .into_iter()
            .find(|plan| matches!(plan.mount, SurfaceMountSpec::Browser(_)))
            .expect("browser surface");

        core.apply_host_event(HostEvent::SurfaceTitleChanged {
            surface_id: browser_surface.surface_id,
            title: "Taskers Docs".into(),
        });
        core.apply_host_event(HostEvent::SurfaceUrlChanged {
            surface_id: browser_surface.surface_id,
            url: "https://example.com/docs".into(),
        });

        let snapshot = core.snapshot();
        let pane = match &snapshot.current_workspace.layout {
            super::LayoutNodeSnapshot::Split { first, second, .. } => [first.as_ref(), second.as_ref()]
                .into_iter()
                .find_map(|node| match node {
                    super::LayoutNodeSnapshot::Pane(pane) => pane
                        .surfaces
                        .iter()
                        .any(|surface| surface.id == browser_surface.surface_id)
                        .then_some(pane),
                    _ => None,
                })
                .expect("browser pane"),
            super::LayoutNodeSnapshot::Pane(_) => panic!("expected split layout"),
        };

        let surface = pane
            .surfaces
            .iter()
            .find(|surface| surface.id == browser_surface.surface_id)
            .expect("browser surface");
        assert_eq!(surface.title, "Taskers Docs");
        assert_eq!(surface.url.as_deref(), Some("https://example.com/docs"));
    }

    #[test]
    fn local_shell_state_revisions_advance_without_app_mutation() {
        let core = SharedCore::bootstrap(bootstrap());
        let before = core.revision();

        core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Settings,
        });

        assert!(core.revision() > before);
        assert!(matches!(core.snapshot().section, ShellSection::Settings));
    }
}
