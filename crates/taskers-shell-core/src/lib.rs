use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    path::PathBuf,
    sync::Arc,
};
use taskers_control::{ControlCommand, ControlResponse};
use taskers_core::{AppState, default_session_path};
use taskers_domain::{
    ActivityItem, AppModel, DEFAULT_WORKSPACE_WINDOW_GAP, KEYBOARD_RESIZE_STEP,
    MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_WIDTH, NotificationId, PaneKind,
    PaneMetadata, PaneMetadataPatch, SplitAxis as DomainSplitAxis, SurfaceRecord, WindowFrame,
    Workspace, WorkspaceSummary as DomainWorkspaceSummary,
};
use taskers_ghostty::{BackendChoice, SurfaceDescriptor};
use taskers_runtime::ShellLaunchSpec;
use time::OffsetDateTime;
use tokio::sync::watch;

pub use taskers_domain::{
    Direction, PaneId, SurfaceId, WorkspaceColumnId, WorkspaceId, WorkspaceWindowId,
    WorkspaceWindowMoveTarget,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActivityId {
    pub notification_id: NotificationId,
}

impl fmt::Display for ActivityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "activity-{}", self.notification_id)
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
pub enum WorkspaceDirection {
    Left,
    Right,
    Up,
    Down,
}

impl WorkspaceDirection {
    fn to_domain(self) -> taskers_domain::Direction {
        match self {
            Self::Left => taskers_domain::Direction::Left,
            Self::Right => taskers_domain::Direction::Right,
            Self::Up => taskers_domain::Direction::Up,
            Self::Down => taskers_domain::Direction::Down,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttentionRingState {
    Waiting,
    Error,
    Completed,
}

impl AttentionRingState {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Error => "error",
            Self::Completed => "completed",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    ToggleOverview,
    FocusLatestUnread,
    CloseTerminal,
    OpenBrowserSplit,
    FocusBrowserAddress,
    ReloadBrowserPage,
    ToggleBrowserDevtools,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    NewWindowLeft,
    NewWindowRight,
    NewWindowUp,
    NewWindowDown,
    MoveWindowLeft,
    MoveWindowRight,
    MoveWindowUp,
    MoveWindowDown,
    ResizeWindowLeft,
    ResizeWindowRight,
    ResizeWindowUp,
    ResizeWindowDown,
    ResizeSplitLeft,
    ResizeSplitRight,
    ResizeSplitUp,
    ResizeSplitDown,
    SplitRight,
    SplitDown,
}

impl ShortcutAction {
    pub const ALL: [Self; 29] = [
        Self::ToggleOverview,
        Self::FocusLatestUnread,
        Self::CloseTerminal,
        Self::OpenBrowserSplit,
        Self::FocusBrowserAddress,
        Self::ReloadBrowserPage,
        Self::ToggleBrowserDevtools,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::NewWindowLeft,
        Self::NewWindowRight,
        Self::NewWindowUp,
        Self::NewWindowDown,
        Self::MoveWindowLeft,
        Self::MoveWindowRight,
        Self::MoveWindowUp,
        Self::MoveWindowDown,
        Self::ResizeWindowLeft,
        Self::ResizeWindowRight,
        Self::ResizeWindowUp,
        Self::ResizeWindowDown,
        Self::ResizeSplitLeft,
        Self::ResizeSplitRight,
        Self::ResizeSplitUp,
        Self::ResizeSplitDown,
        Self::SplitRight,
        Self::SplitDown,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::ToggleOverview => "toggle_overview",
            Self::FocusLatestUnread => "focus_latest_unread",
            Self::CloseTerminal => "close_terminal",
            Self::OpenBrowserSplit => "open_browser_split",
            Self::FocusBrowserAddress => "focus_browser_address",
            Self::ReloadBrowserPage => "reload_browser_page",
            Self::ToggleBrowserDevtools => "toggle_browser_devtools",
            Self::FocusLeft => "focus_left",
            Self::FocusRight => "focus_right",
            Self::FocusUp => "focus_up",
            Self::FocusDown => "focus_down",
            Self::NewWindowLeft => "new_window_left",
            Self::NewWindowRight => "new_window_right",
            Self::NewWindowUp => "new_window_up",
            Self::NewWindowDown => "new_window_down",
            Self::MoveWindowLeft => "move_window_left",
            Self::MoveWindowRight => "move_window_right",
            Self::MoveWindowUp => "move_window_up",
            Self::MoveWindowDown => "move_window_down",
            Self::ResizeWindowLeft => "resize_window_left",
            Self::ResizeWindowRight => "resize_window_right",
            Self::ResizeWindowUp => "resize_window_up",
            Self::ResizeWindowDown => "resize_window_down",
            Self::ResizeSplitLeft => "resize_split_left",
            Self::ResizeSplitRight => "resize_split_right",
            Self::ResizeSplitUp => "resize_split_up",
            Self::ResizeSplitDown => "resize_split_down",
            Self::SplitRight => "split_right",
            Self::SplitDown => "split_down",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Toggle overview",
            Self::FocusLatestUnread => "Jump to latest unread",
            Self::CloseTerminal => "Close terminal",
            Self::OpenBrowserSplit => "Open browser in split",
            Self::FocusBrowserAddress => "Focus browser address bar",
            Self::ReloadBrowserPage => "Reload browser page",
            Self::ToggleBrowserDevtools => "Toggle browser devtools",
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::NewWindowLeft => "New window left",
            Self::NewWindowRight => "New window right",
            Self::NewWindowUp => "New window up",
            Self::NewWindowDown => "New window down",
            Self::MoveWindowLeft => "Move window left",
            Self::MoveWindowRight => "Move window right",
            Self::MoveWindowUp => "Move window up",
            Self::MoveWindowDown => "Move window down",
            Self::ResizeWindowLeft => "Make window narrower",
            Self::ResizeWindowRight => "Make window wider",
            Self::ResizeWindowUp => "Make window shorter",
            Self::ResizeWindowDown => "Make window taller",
            Self::ResizeSplitLeft => "Make split narrower",
            Self::ResizeSplitRight => "Make split wider",
            Self::ResizeSplitUp => "Make split shorter",
            Self::ResizeSplitDown => "Make split taller",
            Self::SplitRight => "Split right",
            Self::SplitDown => "Split down",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Zoom the current workspace out to fit the full column strip.",
            Self::FocusLatestUnread => {
                "Focus the most recent unread attention item in the current app window."
            }
            Self::CloseTerminal => "Close the active pane.",
            Self::OpenBrowserSplit => {
                "Split the active pane to the right and open a browser surface."
            }
            Self::FocusBrowserAddress => "Focus the address bar for the active browser surface.",
            Self::ReloadBrowserPage => "Reload the active browser surface.",
            Self::ToggleBrowserDevtools => "Show or hide devtools for the active browser surface.",
            Self::FocusLeft => {
                "Move focus to the nearest pane on the left before falling back to another window."
            }
            Self::FocusRight => {
                "Move focus to the nearest pane on the right before falling back to another window."
            }
            Self::FocusUp => {
                "Move focus to the nearest pane above before falling back to another window."
            }
            Self::FocusDown => {
                "Move focus to the nearest pane below before falling back to another window."
            }
            Self::NewWindowLeft => "Create a top-level window in a new column on the left.",
            Self::NewWindowRight => "Create a top-level window in a new column on the right.",
            Self::NewWindowUp => "Create a stacked top-level window above the active window.",
            Self::NewWindowDown => "Create a stacked top-level window below the active window.",
            Self::MoveWindowLeft => "Move the active top-level window into the column on the left.",
            Self::MoveWindowRight => {
                "Move the active top-level window into the column on the right."
            }
            Self::MoveWindowUp => "Move the active top-level window above the current stack.",
            Self::MoveWindowDown => "Move the active top-level window below the current stack.",
            Self::ResizeWindowLeft => "Reduce the active column width.",
            Self::ResizeWindowRight => "Increase the active column width.",
            Self::ResizeWindowUp => "Reduce the active top-level window height.",
            Self::ResizeWindowDown => "Increase the active top-level window height.",
            Self::ResizeSplitLeft => "Reduce the active split width.",
            Self::ResizeSplitRight => "Increase the active split width.",
            Self::ResizeSplitUp => "Reduce the active split height.",
            Self::ResizeSplitDown => "Increase the active split height.",
            Self::SplitRight => "Split the active pane to the right inside the current window.",
            Self::SplitDown => "Split the active pane downward inside the current window.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::ToggleOverview | Self::FocusLatestUnread | Self::CloseTerminal => "General",
            Self::OpenBrowserSplit
            | Self::FocusBrowserAddress
            | Self::ReloadBrowserPage
            | Self::ToggleBrowserDevtools => "Browser",
            Self::FocusLeft | Self::FocusRight | Self::FocusUp | Self::FocusDown => "Focus",
            Self::NewWindowLeft
            | Self::NewWindowRight
            | Self::NewWindowUp
            | Self::NewWindowDown
            | Self::MoveWindowLeft
            | Self::MoveWindowRight
            | Self::MoveWindowUp
            | Self::MoveWindowDown => "Top-level windows",
            Self::SplitRight | Self::SplitDown => "Pane splits",
            Self::ResizeWindowLeft
            | Self::ResizeWindowRight
            | Self::ResizeWindowUp
            | Self::ResizeWindowDown
            | Self::ResizeSplitLeft
            | Self::ResizeSplitRight
            | Self::ResizeSplitUp
            | Self::ResizeSplitDown => "Advanced resize",
        }
    }

    pub fn accelerators(self, preset: ShortcutPreset) -> &'static [&'static str] {
        match preset {
            ShortcutPreset::Balanced => match self {
                Self::ToggleOverview => &["<Control><Alt>o"],
                Self::FocusLatestUnread => &["<Control><Shift>u"],
                Self::CloseTerminal => &["<Control><Alt>x"],
                Self::OpenBrowserSplit => &["<Control><Alt><Shift>l"],
                Self::FocusBrowserAddress => &["<Control>l"],
                Self::ReloadBrowserPage => &["<Control>r"],
                Self::ToggleBrowserDevtools => &["<Control><Shift>i"],
                Self::FocusLeft => &["<Control><Alt>h", "<Control><Alt>Left"],
                Self::FocusRight => &["<Control><Alt>l", "<Control><Alt>Right"],
                Self::FocusUp => &["<Control><Alt>k", "<Control><Alt>Up"],
                Self::FocusDown => &["<Control><Alt>j", "<Control><Alt>Down"],
                Self::NewWindowLeft => &[],
                Self::NewWindowRight => &["<Control><Alt>t"],
                Self::NewWindowUp => &[],
                Self::NewWindowDown => &["<Control><Alt>g"],
                Self::MoveWindowLeft => &[],
                Self::MoveWindowRight => &[],
                Self::MoveWindowUp => &[],
                Self::MoveWindowDown => &[],
                Self::ResizeWindowLeft => &[],
                Self::ResizeWindowRight => &[],
                Self::ResizeWindowUp => &[],
                Self::ResizeWindowDown => &[],
                Self::ResizeSplitLeft => &[],
                Self::ResizeSplitRight => &[],
                Self::ResizeSplitUp => &[],
                Self::ResizeSplitDown => &[],
                Self::SplitRight => &["<Control><Alt><Shift>t"],
                Self::SplitDown => &["<Control><Alt><Shift>g"],
            },
            ShortcutPreset::PowerUser => match self {
                Self::ToggleOverview => &["<Control><Alt>o"],
                Self::FocusLatestUnread => &["<Control><Shift>u"],
                Self::CloseTerminal => &["<Control><Alt>x"],
                Self::OpenBrowserSplit => &["<Control><Alt><Shift>l"],
                Self::FocusBrowserAddress => &["<Control>l"],
                Self::ReloadBrowserPage => &["<Control>r"],
                Self::ToggleBrowserDevtools => &["<Control><Shift>i"],
                Self::FocusLeft => &["<Control><Alt>h", "<Control><Alt>Left"],
                Self::FocusRight => &["<Control><Alt>l", "<Control><Alt>Right"],
                Self::FocusUp => &["<Control><Alt>k", "<Control><Alt>Up"],
                Self::FocusDown => &["<Control><Alt>j", "<Control><Alt>Down"],
                Self::NewWindowLeft => &[],
                Self::NewWindowRight => &["<Control><Alt>t"],
                Self::NewWindowUp => &[],
                Self::NewWindowDown => &["<Control><Alt>g"],
                Self::MoveWindowLeft => &["<Control><Alt><Shift>h", "<Control><Alt><Shift>Left"],
                Self::MoveWindowRight => &["<Control><Alt><Shift>l", "<Control><Alt><Shift>Right"],
                Self::MoveWindowUp => &["<Control><Alt><Shift>k", "<Control><Alt><Shift>Up"],
                Self::MoveWindowDown => &["<Control><Alt><Shift>j", "<Control><Alt><Shift>Down"],
                Self::ResizeWindowLeft => &["<Control><Alt>Home"],
                Self::ResizeWindowRight => &["<Control><Alt>End"],
                Self::ResizeWindowUp => &["<Control><Alt>Page_Up"],
                Self::ResizeWindowDown => &["<Control><Alt>Page_Down"],
                Self::ResizeSplitLeft => &["<Control><Alt><Shift>Home"],
                Self::ResizeSplitRight => &["<Control><Alt><Shift>End"],
                Self::ResizeSplitUp => &["<Control><Alt><Shift>Page_Up"],
                Self::ResizeSplitDown => &["<Control><Alt><Shift>Page_Down"],
                Self::SplitRight => &["<Control><Alt><Shift>t"],
                Self::SplitDown => &["<Control><Alt><Shift>g"],
            },
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
    pub notification_preferences: NotificationPreferencesSnapshot,
}

impl Default for BootstrapModel {
    fn default() -> Self {
        Self {
            app_state: default_preview_app_state(),
            runtime_status: RuntimeStatus::default(),
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: ShortcutPreset::Balanced,
            notification_preferences: NotificationPreferencesSnapshot::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationPreferencesSnapshot {
    pub alerts_on_waiting: bool,
    pub alerts_on_error: bool,
    pub alerts_on_completed: bool,
    pub suppress_when_visible: bool,
}

impl Default for NotificationPreferencesSnapshot {
    fn default() -> Self {
        Self {
            alerts_on_waiting: true,
            alerts_on_error: true,
            alerts_on_completed: true,
            suppress_when_visible: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPreferenceKey {
    AlertsOnWaiting,
    AlertsOnError,
    AlertsOnCompleted,
    SuppressWhenVisible,
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

    pub fn inset(self, amount: i32) -> Self {
        let clamped = amount.clamp(0, self.width.min(self.height).saturating_sub(1) / 2);
        Self {
            x: self.x + clamped,
            y: self.y + clamped,
            width: (self.width - clamped * 2).max(1),
            height: (self.height - clamped * 2).max(1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutMetrics {
    pub sidebar_width: i32,
    pub activity_width: i32,
    pub toolbar_height: i32,
    pub workspace_padding: i32,
    pub window_border_width: i32,
    pub window_toolbar_height: i32,
    pub window_body_padding: i32,
    pub split_gap: i32,
    pub pane_border_width: i32,
    pub pane_header_height: i32,
    pub browser_toolbar_height: i32,
    pub surface_tab_height: i32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            sidebar_width: 248,
            activity_width: 312,
            toolbar_height: 42,
            workspace_padding: 16,
            window_border_width: 2,
            window_toolbar_height: 20,
            window_body_padding: 0,
            split_gap: 8,
            pane_border_width: 1,
            pane_header_height: 26,
            browser_toolbar_height: 34,
            surface_tab_height: 28,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStateSnapshot {
    Idle,
    Working,
    Waiting,
    Completed,
    Failed,
}

impl RuntimeStateSnapshot {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Working => "Working",
            Self::Waiting => "Waiting",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeIdentitySnapshot {
    pub key: String,
    pub label: String,
    pub state: RuntimeStateSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceSnapshot {
    pub id: SurfaceId,
    pub kind: SurfaceKind,
    pub runtime: RuntimeIdentitySnapshot,
    pub title: String,
    pub activity_label: Option<String>,
    pub status_label: Option<String>,
    pub url: Option<String>,
    pub cwd: Option<String>,
    pub attention: AttentionState,
    pub notification_ring: Option<AttentionRingState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub active: bool,
    pub attention: AttentionState,
    pub notification_ring: Option<AttentionRingState>,
    pub active_surface: SurfaceId,
    pub runtime: RuntimeIdentitySnapshot,
    pub surfaces: Vec<SurfaceSnapshot>,
    pub focus_flash_token: u64,
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
pub struct WorkspaceSummary {
    pub id: WorkspaceId,
    pub title: String,
    pub preview: String,
    pub active: bool,
    pub runtime: RuntimeIdentitySnapshot,
    pub pane_count: usize,
    pub surface_count: usize,
    pub agent_count: usize,
    pub waiting_agent_count: usize,
    pub unread_activity: usize,
    pub attention: AttentionState,
    pub notification_text: Option<String>,
    pub status_text: Option<String>,
    pub git_branch: Option<String>,
    pub working_directory: Option<String>,
    pub listening_ports: Vec<u16>,
    pub custom_color: Option<String>,
    pub progress: Option<ProgressSnapshot>,
    pub pull_requests: Vec<PullRequestSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressSnapshot {
    pub fraction: f32,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLogEntrySnapshot {
    pub source: Option<String>,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserSurfaceCatalogEntry {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSurfaceCatalogEntry {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub spec: TerminalMountSpec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestSnapshot {
    pub number: u32,
    pub title: String,
    pub status: String,
    pub status_icon: &'static str,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceViewSnapshot {
    pub id: WorkspaceId,
    pub title: String,
    pub attention: AttentionState,
    pub pane_count: usize,
    pub surface_count: usize,
    pub active_window_id: WorkspaceWindowId,
    pub viewport_origin_x: i32,
    pub viewport_origin_y: i32,
    pub active_pane: PaneId,
    pub viewport_x: i32,
    pub viewport_y: i32,
    pub overview_scale: f32,
    pub canvas_width: i32,
    pub canvas_height: i32,
    pub canvas_offset_x: i32,
    pub canvas_offset_y: i32,
    pub columns: Vec<WorkspaceColumnSnapshot>,
    pub layout: LayoutNodeSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceColumnSnapshot {
    pub id: WorkspaceColumnId,
    pub active: bool,
    pub width: i32,
    pub windows: Vec<WorkspaceWindowSnapshot>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceWindowSnapshot {
    pub id: WorkspaceWindowId,
    pub column_id: WorkspaceColumnId,
    pub active: bool,
    pub attention: AttentionState,
    pub runtime: RuntimeIdentitySnapshot,
    pub title: String,
    pub pane_count: usize,
    pub surface_count: usize,
    pub active_pane: PaneId,
    pub frame: Frame,
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
    pub timestamp: String,
    pub body: Option<String>,
    pub source_workspace_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserChromeSnapshot {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub title: String,
    pub url: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub devtools_open: bool,
}

pub const DEFAULT_BROWSER_HOME: &str = "https://duckduckgo.com/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellDragMode {
    #[default]
    None,
    Window,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceDragSessionSnapshot {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub preview_workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStateSnapshot {
    Working,
    Waiting,
    Completed,
    Failed,
}

impl AgentStateSnapshot {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Waiting => "Waiting",
            Self::Completed => "Completed",
            Self::Failed => "Failed",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Working => "busy",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "error",
        }
    }
}

impl From<taskers_domain::WorkspaceAgentState> for AgentStateSnapshot {
    fn from(value: taskers_domain::WorkspaceAgentState) -> Self {
        match value {
            taskers_domain::WorkspaceAgentState::Working => Self::Working,
            taskers_domain::WorkspaceAgentState::Waiting => Self::Waiting,
            taskers_domain::WorkspaceAgentState::Completed => Self::Completed,
            taskers_domain::WorkspaceAgentState::Failed => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionSnapshot {
    pub workspace_id: WorkspaceId,
    pub workspace_title: String,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub agent_kind: String,
    pub title: String,
    pub state: AgentStateSnapshot,
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
    pub notification_preferences: NotificationPreferencesSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSnapshot {
    pub revision: u64,
    pub section: ShellSection,
    pub overview_mode: bool,
    pub drag_mode: ShellDragMode,
    pub surface_drag: Option<SurfaceDragSessionSnapshot>,
    pub attention_panel_visible: bool,
    pub workspaces: Vec<WorkspaceSummary>,
    pub current_workspace: WorkspaceViewSnapshot,
    pub browser_chrome: Option<BrowserChromeSnapshot>,
    pub agents: Vec<AgentSessionSnapshot>,
    pub activity: Vec<ActivityItemSnapshot>,
    pub done_activity: Vec<ActivityItemSnapshot>,
    pub current_workspace_status: Option<String>,
    pub current_workspace_progress: Option<ProgressSnapshot>,
    pub current_workspace_log: Vec<WorkspaceLogEntrySnapshot>,
    pub browser_catalog: Vec<BrowserSurfaceCatalogEntry>,
    pub terminal_catalog: Vec<TerminalSurfaceCatalogEntry>,
    pub portal: SurfacePortalPlan,
    pub metrics: LayoutMetrics,
    pub runtime_status: RuntimeStatus,
    pub settings: SettingsSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostEvent {
    PaneFocused {
        pane_id: PaneId,
    },
    ViewportScrolled {
        dx: i32,
        dy: i32,
    },
    SurfaceClosed {
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    SurfaceTitleChanged {
        surface_id: SurfaceId,
        title: String,
    },
    SurfaceUrlChanged {
        surface_id: SurfaceId,
        url: String,
    },
    SurfaceCwdChanged {
        surface_id: SurfaceId,
        cwd: String,
    },
    BrowserNavigationStateChanged {
        surface_id: SurfaceId,
        can_go_back: bool,
        can_go_forward: bool,
        devtools_open: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCommand {
    BrowserNavigate { surface_id: SurfaceId, url: String },
    BrowserBack { surface_id: SurfaceId },
    BrowserForward { surface_id: SurfaceId },
    BrowserReload { surface_id: SurfaceId },
    BrowserToggleDevtools { surface_id: SurfaceId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellAction {
    ShowSection {
        section: ShellSection,
    },
    ToggleOverview,
    FocusWorkspace {
        workspace_id: WorkspaceId,
    },
    CloseWorkspace {
        workspace_id: WorkspaceId,
    },
    ReorderWorkspaces {
        workspace_ids: Vec<WorkspaceId>,
    },
    CreateWorkspace,
    CreateWorkspaceWindow {
        direction: WorkspaceDirection,
    },
    FocusWorkspaceWindow {
        window_id: WorkspaceWindowId,
    },
    MoveWorkspaceWindow {
        window_id: WorkspaceWindowId,
        target: WorkspaceWindowMoveTarget,
    },
    ScrollViewport {
        dx: i32,
        dy: i32,
    },
    SplitBrowser {
        pane_id: Option<PaneId>,
    },
    SplitTerminal {
        pane_id: Option<PaneId>,
    },
    AddBrowserSurface {
        pane_id: Option<PaneId>,
    },
    AddTerminalSurface {
        pane_id: Option<PaneId>,
    },
    FocusPane {
        pane_id: PaneId,
    },
    FocusSurface {
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    MoveSurface {
        surface_id: SurfaceId,
        target_pane_id: PaneId,
        target_index: usize,
    },
    MoveSurfaceToSplit {
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_pane_id: PaneId,
        direction: Direction,
    },
    MoveSurfaceToWorkspace {
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_workspace_id: WorkspaceId,
    },
    BeginWindowDrag,
    BeginSurfaceDrag {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    PreviewSurfaceDragWorkspace {
        workspace_id: WorkspaceId,
    },
    CancelSurfaceDrag,
    EndDrag,
    NavigateBrowser {
        surface_id: SurfaceId,
        url: String,
    },
    FocusLatestUnread,
    BrowserBack {
        surface_id: SurfaceId,
    },
    BrowserForward {
        surface_id: SurfaceId,
    },
    BrowserReload {
        surface_id: SurfaceId,
    },
    ToggleBrowserDevtools {
        surface_id: SurfaceId,
    },
    CloseSurface {
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    OpenActivity {
        activity_id: ActivityId,
    },
    DismissActivity {
        activity_id: ActivityId,
    },
    SelectTheme {
        theme_id: String,
    },
    SelectShortcutPreset {
        preset_id: String,
    },
    SetNotificationPreference {
        key: NotificationPreferenceKey,
        enabled: bool,
    },
}

#[derive(Debug, Clone)]
struct UiState {
    section: ShellSection,
    overview_mode: bool,
    drag_mode: ShellDragMode,
    surface_drag: Option<SurfaceDragSessionSnapshot>,
    selected_theme_id: String,
    selected_shortcut_preset: ShortcutPreset,
    notification_preferences: NotificationPreferencesSnapshot,
    window_size: PixelSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct BrowserNavigationState {
    can_go_back: bool,
    can_go_forward: bool,
    devtools_open: bool,
}

#[derive(Clone, Copy)]
struct WorkspaceWindowPlacement {
    window_id: WorkspaceWindowId,
    column_id: WorkspaceColumnId,
    frame: WindowFrame,
}

#[derive(Clone, Copy)]
struct CanvasMetrics {
    offset_x: i32,
    offset_y: i32,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy)]
struct WorkspaceRenderContext {
    overview_mode: bool,
    overview_scale: f64,
    outer_padding: i32,
    viewport_width: i32,
    viewport_height: i32,
}

#[derive(Clone)]
struct TaskersCore {
    app_state: AppState,
    revision: u64,
    observed_app_revision: u64,
    metrics: LayoutMetrics,
    runtime_status: RuntimeStatus,
    ui: UiState,
    host_commands: VecDeque<HostCommand>,
    browser_navigation: BTreeMap<SurfaceId, BrowserNavigationState>,
}

impl TaskersCore {
    fn with_bootstrap(bootstrap: BootstrapModel) -> Self {
        let observed_app_revision = bootstrap.app_state.revision();
        let revision = observed_app_revision.max(1);
        Self {
            app_state: bootstrap.app_state,
            revision,
            observed_app_revision,
            metrics: LayoutMetrics::default(),
            runtime_status: bootstrap.runtime_status,
            ui: UiState {
                section: ShellSection::Workspace,
                overview_mode: false,
                drag_mode: ShellDragMode::None,
                surface_drag: None,
                selected_theme_id: bootstrap.selected_theme_id,
                selected_shortcut_preset: bootstrap.selected_shortcut_preset,
                notification_preferences: bootstrap.notification_preferences,
                window_size: PixelSize::new(1440, 900),
            },
            host_commands: VecDeque::new(),
            browser_navigation: BTreeMap::new(),
        }
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn snapshot(&self) -> ShellSnapshot {
        let model = self.app_state.snapshot_model();
        let agents = self.agent_sessions_snapshot(&model);
        let activity = self.activity_snapshot(&model);
        let done_activity = self.done_activity_snapshot(&model);
        let attention_panel_visible = !agents.is_empty() || !activity.is_empty();
        let workspace_id = model
            .active_workspace_id()
            .expect("active workspace should exist");
        let workspace = model
            .workspaces
            .get(&workspace_id)
            .expect("active workspace should exist");
        let current_workspace_progress = workspace_progress_snapshot(workspace);
        let current_workspace_log = workspace
            .log_entries
            .iter()
            .rev()
            .take(12)
            .map(|entry| WorkspaceLogEntrySnapshot {
                source: entry.source.clone(),
                message: entry.message.clone(),
                timestamp: format_relative_time(entry.created_at),
            })
            .collect::<Vec<_>>();
        let active_window = workspace
            .active_window_record()
            .expect("active workspace window should exist");
        let viewport = self.workspace_viewport_frame(attention_panel_visible);
        let clamped_viewport = clamped_workspace_viewport(
            workspace,
            viewport.width,
            viewport.height,
            workspace.viewport.clone(),
        );
        let render_context = workspace_render_context(
            workspace,
            self.ui.overview_mode,
            viewport.width,
            viewport.height,
            self.metrics,
        );
        let placements = workspace_display_window_placements(workspace, render_context);
        let canvas_metrics = workspace_canvas_metrics(&placements, render_context.outer_padding);
        let window_frames = placements
            .iter()
            .map(|placement| {
                (
                    placement.window_id,
                    (
                        placement.column_id,
                        display_window_frame(
                            placement.frame,
                            canvas_metrics,
                            viewport,
                            clamped_viewport.x,
                            clamped_viewport.y,
                            render_context.overview_mode,
                        ),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>();

        ShellSnapshot {
            revision: self.revision,
            section: self.ui.section,
            overview_mode: self.ui.overview_mode,
            drag_mode: self.ui.drag_mode,
            surface_drag: self.ui.surface_drag,
            attention_panel_visible,
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
                active_window_id: workspace.active_window,
                viewport_origin_x: viewport.x,
                viewport_origin_y: viewport.y,
                active_pane: workspace.active_pane,
                viewport_x: clamped_viewport.x,
                viewport_y: clamped_viewport.y,
                overview_scale: render_context.overview_scale as f32,
                canvas_width: canvas_metrics.width,
                canvas_height: canvas_metrics.height,
                canvas_offset_x: canvas_metrics.offset_x,
                canvas_offset_y: canvas_metrics.offset_y,
                columns: self.workspace_columns_snapshot(workspace, &window_frames),
                layout: self.snapshot_layout(workspace, &active_window.layout),
            },
            browser_chrome: self.browser_chrome_snapshot(workspace),
            agents,
            activity,
            done_activity,
            current_workspace_status: workspace.status_text.clone(),
            current_workspace_progress,
            current_workspace_log,
            browser_catalog: self.browser_catalog_snapshot(&model),
            terminal_catalog: self.terminal_catalog_snapshot(&model),
            portal: SurfacePortalPlan {
                window: Frame::new(0, 0, self.ui.window_size.width, self.ui.window_size.height),
                content: viewport,
                panes: if matches!(self.ui.section, ShellSection::Workspace)
                    && !self.ui.overview_mode
                {
                    self.collect_workspace_surface_plans(workspace_id, workspace, &window_frames)
                } else {
                    Vec::new()
                },
            },
            metrics: self.metrics,
            runtime_status: self.runtime_status.clone(),
            settings: self.settings_snapshot(),
        }
    }

    fn workspace_viewport_frame(&self, attention_panel_visible: bool) -> Frame {
        let metrics = self.metrics;
        let activity_width = if attention_panel_visible {
            metrics.activity_width
        } else {
            0
        };
        let width = (self.ui.window_size.width - metrics.sidebar_width - activity_width).max(640);
        Frame::new(
            metrics.sidebar_width,
            metrics.toolbar_height,
            width,
            (self.ui.window_size.height - metrics.toolbar_height).max(220),
        )
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
            notification_preferences: self.ui.notification_preferences,
        }
    }

    fn workspace_summaries(&self, model: &AppModel) -> Vec<WorkspaceSummary> {
        let active_window = model.active_window;
        let now = OffsetDateTime::now_utc();
        model
            .workspace_summaries(active_window)
            .unwrap_or_default()
            .into_iter()
            .map(|summary| {
                let workspace = model.workspaces.get(&summary.workspace_id);
                let active_pane_surface = workspace
                    .and_then(|ws| ws.panes.get(&summary.active_pane))
                    .and_then(|pane| pane.active_surface());
                let git_branch =
                    active_pane_surface.and_then(|surface| surface.metadata.git_branch.clone());
                let working_directory =
                    active_pane_surface.and_then(|surface| surface.metadata.cwd.clone());
                let mut listening_ports: Vec<u16> = workspace
                    .into_iter()
                    .flat_map(|ws| ws.panes.values())
                    .flat_map(|pane| pane.surfaces.values())
                    .flat_map(|surface| surface.metadata.ports.iter().copied())
                    .collect();
                listening_ports.sort_unstable();
                listening_ports.dedup();

                WorkspaceSummary {
                    id: summary.workspace_id,
                    title: summary.label.clone(),
                    preview: workspace_preview(&summary),
                    active: model.active_workspace_id() == Some(summary.workspace_id),
                    runtime: workspace_runtime_identity(workspace, now),
                    pane_count: workspace.map(|ws| ws.panes.len()).unwrap_or_default(),
                    surface_count: workspace.map(workspace_surface_count).unwrap_or_default(),
                    agent_count: summary.agent_summaries.len(),
                    waiting_agent_count: summary
                        .agent_summaries
                        .iter()
                        .filter(|agent| {
                            matches!(agent.state, taskers_domain::WorkspaceAgentState::Waiting)
                        })
                        .count(),
                    unread_activity: summary.unread_count,
                    attention: summary.display_attention.into(),
                    notification_text: summary.latest_notification,
                    status_text: summary.status_text,
                    git_branch,
                    working_directory,
                    listening_ports,
                    custom_color: workspace.and_then(|ws| ws.custom_color.clone()),
                    progress: workspace.and_then(workspace_progress_snapshot),
                    pull_requests: workspace
                        .into_iter()
                        .flat_map(|ws| ws.panes.values())
                        .flat_map(|pane| pane.surfaces.values())
                        .flat_map(|surface| surface.metadata.pull_requests.iter())
                        .map(|pr| {
                            let (status, status_icon) = match pr.status {
                                taskers_domain::PrStatus::Open => ("Open", "●"),
                                taskers_domain::PrStatus::Draft => ("Draft", "◐"),
                                taskers_domain::PrStatus::Merged => ("Merged", "✓"),
                                taskers_domain::PrStatus::Closed => ("Closed", "✕"),
                            };
                            PullRequestSnapshot {
                                number: pr.number,
                                title: pr.title.clone(),
                                status: status.into(),
                                status_icon,
                                url: pr.url.clone(),
                            }
                        })
                        .collect(),
                }
            })
            .collect()
    }

    fn agent_sessions_snapshot(&self, model: &AppModel) -> Vec<AgentSessionSnapshot> {
        let active_window = model.active_window;
        model
            .workspace_summaries(active_window)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|summary| {
                summary
                    .agent_summaries
                    .into_iter()
                    .map(move |agent| AgentSessionSnapshot {
                        workspace_id: summary.workspace_id,
                        workspace_title: summary.label.clone(),
                        pane_id: agent.pane_id,
                        surface_id: agent.surface_id,
                        agent_kind: agent.agent_kind.clone(),
                        title: agent.title.clone().unwrap_or_else(|| {
                            format!("{} {}", agent.agent_kind, agent.state.label())
                        }),
                        state: agent.state.into(),
                    })
            })
            .collect()
    }

    fn activity_snapshot(&self, model: &AppModel) -> Vec<ActivityItemSnapshot> {
        model
            .activity_items()
            .into_iter()
            .map(|item| activity_item_snapshot(model, &item))
            .collect()
    }

    fn done_activity_snapshot(&self, model: &AppModel) -> Vec<ActivityItemSnapshot> {
        let mut items = model
            .workspaces
            .values()
            .flat_map(|workspace| {
                workspace
                    .notifications
                    .iter()
                    .filter(|notification| notification.cleared_at.is_some())
                    .map(move |notification| ActivityItem {
                        notification_id: notification.id,
                        workspace_id: workspace.id,
                        workspace_window_id: workspace.window_for_pane(notification.pane_id),
                        pane_id: notification.pane_id,
                        surface_id: notification.surface_id,
                        kind: notification.kind.clone(),
                        state: notification.state,
                        title: notification.title.clone(),
                        subtitle: notification.subtitle.clone(),
                        message: notification.message.clone(),
                        read_at: notification.read_at,
                        created_at: notification.created_at,
                    })
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        items
            .into_iter()
            .map(|item| activity_item_snapshot(model, &item))
            .collect()
    }

    fn workspace_columns_snapshot(
        &self,
        workspace: &Workspace,
        window_frames: &BTreeMap<WorkspaceWindowId, (WorkspaceColumnId, Frame)>,
    ) -> Vec<WorkspaceColumnSnapshot> {
        let now = OffsetDateTime::now_utc();
        let active_column_id = workspace.active_column_id();
        workspace
            .columns
            .values()
            .map(|column| WorkspaceColumnSnapshot {
                id: column.id,
                active: active_column_id == Some(column.id),
                width: column.width,
                windows: column
                    .window_order
                    .iter()
                    .filter_map(|window_id| {
                        let window = workspace.windows.get(window_id)?;
                        let (_, frame) = window_frames.get(window_id)?;
                        Some(
                            self.workspace_window_snapshot(
                                workspace, column.id, window, *frame, now,
                            ),
                        )
                    })
                    .collect(),
            })
            .collect()
    }

    fn workspace_window_snapshot(
        &self,
        workspace: &Workspace,
        column_id: WorkspaceColumnId,
        window: &taskers_domain::WorkspaceWindowRecord,
        frame: Frame,
        now: OffsetDateTime,
    ) -> WorkspaceWindowSnapshot {
        let pane_ids = window.layout.leaves();
        let pane_count = pane_ids.len();
        let surface_count = pane_ids
            .iter()
            .filter_map(|pane_id| workspace.panes.get(pane_id))
            .map(|pane| pane.surfaces.len())
            .sum();
        let title = window_primary_title(workspace, window);

        WorkspaceWindowSnapshot {
            id: window.id,
            column_id,
            active: workspace.active_window == window.id,
            attention: workspace_window_attention(workspace, window),
            runtime: workspace_window_runtime_identity(workspace, window, now),
            title,
            pane_count,
            surface_count,
            active_pane: window.active_pane,
            frame,
            layout: self.snapshot_layout(workspace, &window.layout),
        }
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

    fn pane_snapshot(
        &self,
        workspace: &Workspace,
        pane: &taskers_domain::PaneRecord,
    ) -> PaneSnapshot {
        let now = OffsetDateTime::now_utc();
        let is_active = workspace.active_pane == pane.id;
        let has_unread = pane.highest_attention() != taskers_domain::AttentionState::Normal;
        let explicit_flash_token = pane
            .surfaces
            .values()
            .filter_map(|surface| workspace.surface_flash_tokens.get(&surface.id))
            .copied()
            .max()
            .unwrap_or(0);
        let focus_flash_token = if is_active && has_unread {
            self.revision
        } else {
            0
        };
        let flash_token = focus_flash_token.max(explicit_flash_token);
        let surfaces = pane
            .surfaces
            .values()
            .map(|surface| SurfaceSnapshot {
                id: surface.id,
                kind: SurfaceKind::from_domain(&surface.kind),
                runtime: surface_runtime_identity(surface, now),
                title: display_surface_title(surface),
                activity_label: surface_activity_label(surface, now),
                status_label: surface_status_label(surface, now),
                url: normalized_surface_url(surface),
                cwd: normalized_cwd(&surface.metadata),
                attention: surface.attention.into(),
                notification_ring: surface_notification_ring(surface),
            })
            .collect::<Vec<_>>();
        PaneSnapshot {
            id: pane.id,
            active: is_active,
            attention: pane.highest_attention().into(),
            notification_ring: dominant_attention_ring(
                surfaces
                    .iter()
                    .filter_map(|surface| surface.notification_ring),
            ),
            active_surface: pane.active_surface,
            runtime: pane_runtime_identity(pane, now),
            surfaces,
            focus_flash_token: flash_token,
        }
    }

    fn browser_chrome_snapshot(&self, workspace: &Workspace) -> Option<BrowserChromeSnapshot> {
        let pane = workspace.panes.get(&workspace.active_pane)?;
        let surface = pane.active_surface()?;
        if surface.kind != PaneKind::Browser {
            return None;
        }

        Some(BrowserChromeSnapshot {
            pane_id: pane.id,
            surface_id: surface.id,
            title: display_surface_title(surface),
            url: normalized_surface_url(surface).unwrap_or_else(|| DEFAULT_BROWSER_HOME.into()),
            can_go_back: self
                .browser_navigation
                .get(&surface.id)
                .map(|state| state.can_go_back)
                .unwrap_or(false),
            can_go_forward: self
                .browser_navigation
                .get(&surface.id)
                .map(|state| state.can_go_forward)
                .unwrap_or(false),
            devtools_open: self
                .browser_navigation
                .get(&surface.id)
                .map(|state| state.devtools_open)
                .unwrap_or(false),
        })
    }

    fn browser_catalog_snapshot(&self, model: &AppModel) -> Vec<BrowserSurfaceCatalogEntry> {
        let mut catalog = Vec::new();
        for (workspace_id, workspace) in &model.workspaces {
            for pane in workspace.panes.values() {
                for surface in pane.surfaces.values() {
                    if surface.kind != PaneKind::Browser {
                        continue;
                    }
                    let descriptor = fallback_surface_descriptor(surface);
                    let mount = mount_spec_from_descriptor(surface, descriptor);
                    let SurfaceMountSpec::Browser(BrowserMountSpec { url }) = mount else {
                        continue;
                    };
                    catalog.push(BrowserSurfaceCatalogEntry {
                        workspace_id: *workspace_id,
                        pane_id: pane.id,
                        surface_id: surface.id,
                        url,
                    });
                }
            }
        }
        catalog
    }

    fn terminal_catalog_snapshot(&self, model: &AppModel) -> Vec<TerminalSurfaceCatalogEntry> {
        let mut catalog = Vec::new();
        for (workspace_id, workspace) in &model.workspaces {
            for pane in workspace.panes.values() {
                for surface in pane.surfaces.values() {
                    if surface.kind != PaneKind::Terminal {
                        continue;
                    }
                    let descriptor = fallback_surface_descriptor(surface);
                    let mount = mount_spec_from_descriptor(surface, descriptor);
                    let SurfaceMountSpec::Terminal(spec) = mount else {
                        continue;
                    };
                    catalog.push(TerminalSurfaceCatalogEntry {
                        workspace_id: *workspace_id,
                        pane_id: pane.id,
                        surface_id: surface.id,
                        spec,
                    });
                }
            }
        }
        catalog
    }

    fn collect_workspace_surface_plans(
        &self,
        workspace_id: WorkspaceId,
        workspace: &Workspace,
        window_frames: &BTreeMap<WorkspaceWindowId, (WorkspaceColumnId, Frame)>,
    ) -> Vec<PortalSurfacePlan> {
        workspace
            .windows
            .values()
            .filter_map(|window| {
                let (_, frame) = window_frames.get(&window.id)?;
                Some(self.collect_surface_plans(
                    workspace_id,
                    workspace,
                    &window.layout,
                    workspace_window_content_frame(*frame, self.metrics),
                ))
            })
            .flatten()
            .collect()
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
                        frame: pane_body_frame(
                            frame,
                            self.metrics,
                            &active_surface.kind,
                            pane_shows_tab_strip_for_surface_count(pane.surfaces.len()),
                        ),
                        mount: self.mount_spec_for_active_surface(
                            workspace_id,
                            pane,
                            active_surface,
                        ),
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
                let (first_frame, second_frame) = split_frame(
                    frame,
                    SplitAxis::from_domain(*axis),
                    *ratio,
                    self.metrics.split_gap,
                );
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
            HostEvent::ViewportScrolled { dx, dy } => {
                matches!(self.ui.section, ShellSection::Workspace)
                    && !self.ui.overview_mode
                    && self.scroll_viewport_by(dx, dy)
            }
            HostEvent::SurfaceClosed {
                pane_id,
                surface_id,
            } => {
                self.browser_navigation.remove(&surface_id);
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
            HostEvent::BrowserNavigationStateChanged {
                surface_id,
                can_go_back,
                can_go_forward,
                devtools_open,
            } => {
                let next = BrowserNavigationState {
                    can_go_back,
                    can_go_forward,
                    devtools_open,
                };
                if self.browser_navigation.get(&surface_id) == Some(&next) {
                    return false;
                }
                self.browser_navigation.insert(surface_id, next);
                self.bump_local_revision();
                true
            }
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
            ShellAction::CloseWorkspace { workspace_id } => {
                self.dispatch_control(ControlCommand::CloseWorkspace { workspace_id })
            }
            ShellAction::ReorderWorkspaces { workspace_ids } => {
                let window_id = self.app_state.snapshot_model().active_window;
                self.dispatch_control(ControlCommand::ReorderWorkspaces {
                    window_id,
                    workspace_ids,
                })
            }
            ShellAction::CreateWorkspace => self.create_workspace(),
            ShellAction::CreateWorkspaceWindow { direction } => {
                self.create_workspace_window(direction)
            }
            ShellAction::FocusWorkspaceWindow { window_id } => {
                self.focus_workspace_window(window_id)
            }
            ShellAction::MoveWorkspaceWindow { window_id, target } => {
                self.move_workspace_window_by_id(window_id, target)
            }
            ShellAction::ScrollViewport { dx, dy } => self.scroll_viewport_by(dx, dy),
            ShellAction::SplitBrowser { pane_id } => {
                self.split_with_kind_axis(pane_id, PaneKind::Browser, DomainSplitAxis::Horizontal)
            }
            ShellAction::SplitTerminal { pane_id } => {
                self.split_with_kind_axis(pane_id, PaneKind::Terminal, DomainSplitAxis::Horizontal)
            }
            ShellAction::AddBrowserSurface { pane_id } => {
                self.add_surface_to_pane(pane_id, PaneKind::Browser)
            }
            ShellAction::AddTerminalSurface { pane_id } => {
                self.add_surface_to_pane(pane_id, PaneKind::Terminal)
            }
            ShellAction::FocusPane { pane_id } => self.focus_pane_by_id(pane_id),
            ShellAction::FocusSurface {
                pane_id,
                surface_id,
            } => self.focus_surface_by_id(pane_id, surface_id),
            ShellAction::MoveSurface {
                surface_id,
                target_pane_id,
                target_index,
            } => self.move_surface_by_id(surface_id, target_pane_id, target_index),
            ShellAction::MoveSurfaceToSplit {
                source_pane_id,
                surface_id,
                target_pane_id,
                direction,
            } => self.move_surface_to_split_by_id(
                source_pane_id,
                surface_id,
                target_pane_id,
                direction,
            ),
            ShellAction::MoveSurfaceToWorkspace {
                source_pane_id,
                surface_id,
                target_workspace_id,
            } => self.move_surface_to_workspace_by_id(
                source_pane_id,
                surface_id,
                target_workspace_id,
            ),
            ShellAction::BeginWindowDrag => self.begin_window_drag(),
            ShellAction::BeginSurfaceDrag {
                workspace_id,
                pane_id,
                surface_id,
            } => self.begin_surface_drag(workspace_id, pane_id, surface_id),
            ShellAction::PreviewSurfaceDragWorkspace { workspace_id } => {
                self.preview_surface_drag_workspace(workspace_id)
            }
            ShellAction::CancelSurfaceDrag => self.clear_surface_drag(true),
            ShellAction::EndDrag => self.clear_surface_drag(false),
            ShellAction::NavigateBrowser { surface_id, url } => {
                self.navigate_browser_surface(surface_id, &url)
            }
            ShellAction::FocusLatestUnread => {
                self.dispatch_control(ControlCommand::AgentFocusLatestUnread { window_id: None })
            }
            ShellAction::BrowserBack { surface_id } => {
                self.queue_host_command(HostCommand::BrowserBack { surface_id })
            }
            ShellAction::BrowserForward { surface_id } => {
                self.queue_host_command(HostCommand::BrowserForward { surface_id })
            }
            ShellAction::BrowserReload { surface_id } => {
                self.queue_host_command(HostCommand::BrowserReload { surface_id })
            }
            ShellAction::ToggleBrowserDevtools { surface_id } => {
                self.queue_host_command(HostCommand::BrowserToggleDevtools { surface_id })
            }
            ShellAction::CloseSurface {
                pane_id,
                surface_id,
            } => self.close_surface_by_id(pane_id, surface_id),
            ShellAction::OpenActivity { activity_id } => self.open_activity(activity_id),
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
            ShellAction::SetNotificationPreference { key, enabled } => {
                let changed = match key {
                    NotificationPreferenceKey::AlertsOnWaiting => {
                        &mut self.ui.notification_preferences.alerts_on_waiting
                    }
                    NotificationPreferenceKey::AlertsOnError => {
                        &mut self.ui.notification_preferences.alerts_on_error
                    }
                    NotificationPreferenceKey::AlertsOnCompleted => {
                        &mut self.ui.notification_preferences.alerts_on_completed
                    }
                    NotificationPreferenceKey::SuppressWhenVisible => {
                        &mut self.ui.notification_preferences.suppress_when_visible
                    }
                };
                if *changed == enabled {
                    return false;
                }
                *changed = enabled;
                self.bump_local_revision();
                true
            }
        }
    }

    fn dispatch_shortcut_action(&mut self, action: ShortcutAction) -> bool {
        match action {
            ShortcutAction::ToggleOverview => {
                self.dispatch_shell_action(ShellAction::ToggleOverview)
            }
            ShortcutAction::FocusLatestUnread => {
                self.dispatch_shell_action(ShellAction::FocusLatestUnread)
            }
            ShortcutAction::CloseTerminal => self.run_workspace_shortcut(|core, workspace_id| {
                let pane_id = core
                    .app_state
                    .snapshot_model()
                    .workspaces
                    .get(&workspace_id)
                    .map(|workspace| workspace.active_pane)?;
                Some(core.dispatch_control(ControlCommand::ClosePane {
                    workspace_id,
                    pane_id,
                }))
            }),
            ShortcutAction::OpenBrowserSplit => self.run_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(
                    None,
                    PaneKind::Browser,
                    DomainSplitAxis::Horizontal,
                ))
            }),
            ShortcutAction::FocusBrowserAddress => false,
            ShortcutAction::ReloadBrowserPage => {
                self.with_active_browser_surface(|core, surface_id| {
                    core.queue_host_command(HostCommand::BrowserReload { surface_id })
                })
            }
            ShortcutAction::ToggleBrowserDevtools => {
                self.with_active_browser_surface(|core, surface_id| {
                    core.queue_host_command(HostCommand::BrowserToggleDevtools { surface_id })
                })
            }
            ShortcutAction::FocusLeft => self.run_workspace_shortcut(|core, workspace_id| {
                Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                    workspace_id,
                    direction: Direction::Left,
                }))
            }),
            ShortcutAction::FocusRight => self.run_workspace_shortcut(|core, workspace_id| {
                Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                    workspace_id,
                    direction: Direction::Right,
                }))
            }),
            ShortcutAction::FocusUp => self.run_workspace_shortcut(|core, workspace_id| {
                Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                    workspace_id,
                    direction: Direction::Up,
                }))
            }),
            ShortcutAction::FocusDown => self.run_workspace_shortcut(|core, workspace_id| {
                Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                    workspace_id,
                    direction: Direction::Down,
                }))
            }),
            ShortcutAction::NewWindowLeft => self.run_workspace_shortcut(|core, _| {
                Some(core.create_workspace_window(WorkspaceDirection::Left))
            }),
            ShortcutAction::NewWindowRight => self.run_workspace_shortcut(|core, _| {
                Some(core.create_workspace_window(WorkspaceDirection::Right))
            }),
            ShortcutAction::NewWindowUp => self.run_workspace_shortcut(|core, _| {
                Some(core.create_workspace_window(WorkspaceDirection::Up))
            }),
            ShortcutAction::NewWindowDown => self.run_workspace_shortcut(|core, _| {
                Some(core.create_workspace_window(WorkspaceDirection::Down))
            }),
            ShortcutAction::MoveWindowLeft
            | ShortcutAction::MoveWindowRight
            | ShortcutAction::MoveWindowUp
            | ShortcutAction::MoveWindowDown => self.run_workspace_shortcut(|core, _| {
                let direction = match action {
                    ShortcutAction::MoveWindowLeft => Direction::Left,
                    ShortcutAction::MoveWindowRight => Direction::Right,
                    ShortcutAction::MoveWindowUp => Direction::Up,
                    ShortcutAction::MoveWindowDown => Direction::Down,
                    _ => unreachable!("move action already matched"),
                };
                Some(core.move_active_workspace_window(direction))
            }),
            ShortcutAction::ResizeWindowLeft => {
                self.run_workspace_shortcut(|core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Left,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                })
            }
            ShortcutAction::ResizeWindowRight => {
                self.run_workspace_shortcut(|core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Right,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                })
            }
            ShortcutAction::ResizeWindowUp => self.run_workspace_shortcut(|core, workspace_id| {
                Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                    workspace_id,
                    direction: Direction::Up,
                    amount: KEYBOARD_RESIZE_STEP,
                }))
            }),
            ShortcutAction::ResizeWindowDown => {
                self.run_workspace_shortcut(|core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Down,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                })
            }
            ShortcutAction::ResizeSplitLeft => self.run_workspace_shortcut(|core, workspace_id| {
                Some(
                    core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                        workspace_id,
                        direction: Direction::Left,
                        amount: KEYBOARD_RESIZE_STEP,
                    }),
                )
            }),
            ShortcutAction::ResizeSplitRight => {
                self.run_workspace_shortcut(|core, workspace_id| {
                    Some(
                        core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                            workspace_id,
                            direction: Direction::Right,
                            amount: KEYBOARD_RESIZE_STEP,
                        }),
                    )
                })
            }
            ShortcutAction::ResizeSplitUp => self.run_workspace_shortcut(|core, workspace_id| {
                Some(
                    core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                        workspace_id,
                        direction: Direction::Up,
                        amount: KEYBOARD_RESIZE_STEP,
                    }),
                )
            }),
            ShortcutAction::ResizeSplitDown => self.run_workspace_shortcut(|core, workspace_id| {
                Some(
                    core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                        workspace_id,
                        direction: Direction::Down,
                        amount: KEYBOARD_RESIZE_STEP,
                    }),
                )
            }),
            ShortcutAction::SplitRight => self.run_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(
                    None,
                    PaneKind::Terminal,
                    DomainSplitAxis::Horizontal,
                ))
            }),
            ShortcutAction::SplitDown => self.run_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(None, PaneKind::Terminal, DomainSplitAxis::Vertical))
            }),
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

    fn create_workspace_window(&mut self, direction: WorkspaceDirection) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::CreateWorkspaceWindow {
            workspace_id,
            direction: direction.to_domain(),
        })
    }

    fn focus_workspace_window(&mut self, window_id: WorkspaceWindowId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::FocusWorkspaceWindow {
            workspace_id,
            workspace_window_id: window_id,
        })
    }

    fn move_workspace_window_by_id(
        &mut self,
        window_id: WorkspaceWindowId,
        target: WorkspaceWindowMoveTarget,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::MoveWorkspaceWindow {
            workspace_id,
            workspace_window_id: window_id,
            target,
        })
    }

    fn scroll_viewport_by(&mut self, dx: i32, dy: i32) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let viewport_frame = self.workspace_viewport_frame(attention_panel_visible(&model));
        let current_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            workspace.viewport.clone(),
        );
        let next_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            taskers_domain::WorkspaceViewport {
                x: current_viewport.x.saturating_add(dx),
                y: current_viewport.y.saturating_add(dy),
            },
        );
        if next_viewport == workspace.viewport {
            return false;
        }
        self.dispatch_control(ControlCommand::SetWorkspaceViewport {
            workspace_id,
            viewport: next_viewport,
        })
    }

    fn split_with_kind_axis(
        &mut self,
        pane_id: Option<PaneId>,
        kind: PaneKind,
        axis: DomainSplitAxis,
    ) -> bool {
        let Some((workspace_id, target_pane_id)) = self.resolve_target_pane(pane_id) else {
            return false;
        };

        let response = match self.dispatch_control_with_response(ControlCommand::SplitPane {
            workspace_id,
            pane_id: Some(target_pane_id),
            axis,
        }) {
            Some(response) => response,
            None => return false,
        };

        let ControlResponse::PaneSplit {
            pane_id: new_pane_id,
        } = response
        else {
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
        let Some((workspace_id, _)) =
            self.resolve_workspace_pane(&self.app_state.snapshot_model(), pane_id)
        else {
            return false;
        };
        if self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id) {
            let _ = self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        self.dispatch_control(ControlCommand::FocusPane {
            workspace_id,
            pane_id,
        })
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

    fn move_surface_by_id(
        &mut self,
        surface_id: SurfaceId,
        target_pane_id: PaneId,
        target_index: usize,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some((workspace_id, source_pane_id)) =
            self.resolve_surface_location(&model, surface_id)
        else {
            return false;
        };
        let Some((target_workspace_id, _)) = self.resolve_workspace_pane(&model, target_pane_id)
        else {
            return false;
        };
        if source_pane_id == target_pane_id {
            return self.dispatch_control(ControlCommand::MoveSurface {
                workspace_id,
                pane_id: source_pane_id,
                surface_id,
                to_index: target_index,
            });
        }
        let changed = self
            .dispatch_control_with_response(ControlCommand::TransferSurface {
                source_workspace_id: workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
                target_pane_id,
                to_index: target_index,
            })
            .is_some();
        if changed && workspace_id != target_workspace_id {
            return self.ensure_active_window_visible() || changed;
        }
        changed
    }

    fn move_surface_to_split_by_id(
        &mut self,
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_pane_id: PaneId,
        direction: Direction,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some((source_workspace_id, located_source_pane_id)) =
            self.resolve_surface_location(&model, surface_id)
        else {
            return false;
        };
        let Some((target_workspace_id, _)) = self.resolve_workspace_pane(&model, target_pane_id)
        else {
            return false;
        };
        if located_source_pane_id != source_pane_id {
            return false;
        }
        let Some(response) =
            self.dispatch_control_with_response(ControlCommand::MoveSurfaceToSplit {
                source_workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
                target_pane_id,
                direction,
            })
        else {
            return false;
        };
        let changed = matches!(response, ControlResponse::SurfaceMovedToSplit { .. });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn move_surface_to_workspace_by_id(
        &mut self,
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_workspace_id: WorkspaceId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some((source_workspace_id, located_source_pane_id)) =
            self.resolve_surface_location(&model, surface_id)
        else {
            return false;
        };
        if located_source_pane_id != source_pane_id || source_workspace_id == target_workspace_id {
            return false;
        }
        let Some(response) =
            self.dispatch_control_with_response(ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
            })
        else {
            return false;
        };
        let changed = matches!(response, ControlResponse::SurfaceMovedToWorkspace { .. });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn navigate_browser_surface(&mut self, surface_id: SurfaceId, raw_url: &str) -> bool {
        let normalized = resolved_browser_uri(raw_url);
        let mut changed = self.dispatch_control(ControlCommand::UpdateSurfaceMetadata {
            surface_id,
            patch: PaneMetadataPatch {
                url: Some(normalized.clone()),
                ..PaneMetadataPatch::default()
            },
        });
        changed |= self.queue_host_command(HostCommand::BrowserNavigate {
            surface_id,
            url: normalized,
        });
        changed
    }

    fn queue_host_command(&mut self, command: HostCommand) -> bool {
        self.host_commands.push_back(command);
        true
    }

    fn dismiss_activity(&mut self, activity_id: ActivityId) -> bool {
        self.dispatch_control(ControlCommand::ClearNotification {
            notification_id: activity_id.notification_id,
        })
    }

    fn open_activity(&mut self, activity_id: ActivityId) -> bool {
        self.dispatch_control(ControlCommand::OpenNotification {
            window_id: None,
            notification_id: activity_id.notification_id,
        })
    }

    fn update_surface_metadata(&mut self, surface_id: SurfaceId, patch: PaneMetadataPatch) -> bool {
        self.dispatch_control(ControlCommand::UpdateSurfaceMetadata { surface_id, patch })
    }

    fn move_active_workspace_window(&mut self, direction: Direction) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let active_window_id = workspace.active_window;
        let Some((active_column_id, active_column_index, active_window_index)) = workspace
            .columns
            .iter()
            .enumerate()
            .find_map(|(column_index, (column_id, column))| {
                column
                    .window_order
                    .iter()
                    .position(|candidate| *candidate == active_window_id)
                    .map(|window_index| (*column_id, column_index, window_index))
            })
        else {
            return false;
        };

        let target = match direction {
            Direction::Left => {
                if let Some((column_id, _)) = active_column_index
                    .checked_sub(1)
                    .and_then(|index| workspace.columns.get_index(index))
                {
                    WorkspaceWindowMoveTarget::ColumnBefore {
                        column_id: *column_id,
                    }
                } else if workspace
                    .columns
                    .get(&active_column_id)
                    .is_some_and(|column| column.window_order.len() > 1)
                {
                    WorkspaceWindowMoveTarget::ColumnBefore {
                        column_id: active_column_id,
                    }
                } else {
                    return false;
                }
            }
            Direction::Right => {
                if let Some((column_id, _)) = workspace.columns.get_index(active_column_index + 1) {
                    WorkspaceWindowMoveTarget::ColumnAfter {
                        column_id: *column_id,
                    }
                } else if workspace
                    .columns
                    .get(&active_column_id)
                    .is_some_and(|column| column.window_order.len() > 1)
                {
                    WorkspaceWindowMoveTarget::ColumnAfter {
                        column_id: active_column_id,
                    }
                } else {
                    return false;
                }
            }
            Direction::Up => {
                let Some(window_id) = workspace
                    .columns
                    .get(&active_column_id)
                    .and_then(|column| {
                        active_window_index
                            .checked_sub(1)
                            .and_then(|index| column.window_order.get(index))
                    })
                    .copied()
                else {
                    return false;
                };
                WorkspaceWindowMoveTarget::StackAbove { window_id }
            }
            Direction::Down => {
                let Some(window_id) = workspace
                    .columns
                    .get(&active_column_id)
                    .and_then(|column| column.window_order.get(active_window_index + 1))
                    .copied()
                else {
                    return false;
                };
                WorkspaceWindowMoveTarget::StackBelow { window_id }
            }
        };

        self.move_workspace_window_by_id(active_window_id, target)
    }

    fn run_workspace_shortcut(
        &mut self,
        handler: impl FnOnce(&mut Self, WorkspaceId) -> Option<bool>,
    ) -> bool {
        let Some(workspace_id) = self.prepare_workspace_interaction() else {
            return false;
        };
        let Some(mut changed) = handler(self, workspace_id) else {
            return false;
        };
        changed |= self.ensure_active_window_visible();
        changed
    }

    fn begin_window_drag(&mut self) -> bool {
        let mut changed = false;
        if self.ui.surface_drag.is_some() {
            self.ui.surface_drag = None;
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::Window {
            self.ui.drag_mode = ShellDragMode::Window;
            changed = true;
        }
        if changed {
            self.bump_local_revision();
        }
        changed
    }

    fn begin_surface_drag(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        if self.resolve_surface_location(&model, surface_id) != Some((workspace_id, pane_id)) {
            return false;
        }
        let next = SurfaceDragSessionSnapshot {
            workspace_id,
            pane_id,
            surface_id,
            preview_workspace_id: workspace_id,
        };
        if self.ui.drag_mode == ShellDragMode::Surface && self.ui.surface_drag == Some(next) {
            return false;
        }
        self.ui.drag_mode = ShellDragMode::Surface;
        self.ui.surface_drag = Some(next);
        self.bump_local_revision();
        true
    }

    fn preview_surface_drag_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        let Some(mut session) = self.ui.surface_drag else {
            return false;
        };
        let mut changed = false;
        if session.preview_workspace_id != workspace_id {
            session.preview_workspace_id = workspace_id;
            self.ui.surface_drag = Some(session);
            self.bump_local_revision();
            changed = true;
        }
        if self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id) {
            changed |= self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        changed
    }

    fn clear_surface_drag(&mut self, restore_source_workspace: bool) -> bool {
        let source_workspace_id = self.ui.surface_drag.map(|session| session.workspace_id);
        let mut changed = false;
        if self.ui.surface_drag.is_some() {
            self.ui.surface_drag = None;
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::None {
            self.ui.drag_mode = ShellDragMode::None;
            changed = true;
        }
        if changed {
            self.bump_local_revision();
        }
        if restore_source_workspace
            && let Some(workspace_id) = source_workspace_id
            && self.app_state.snapshot_model().active_workspace_id() != Some(workspace_id)
        {
            changed |= self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        changed
    }

    fn prepare_workspace_interaction(&mut self) -> Option<WorkspaceId> {
        let mut changed = false;
        if self.ui.section != ShellSection::Workspace {
            self.ui.section = ShellSection::Workspace;
            changed = true;
        }
        if self.ui.overview_mode {
            self.ui.overview_mode = false;
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::None {
            self.ui.drag_mode = ShellDragMode::None;
            changed = true;
        }
        if self.ui.surface_drag.is_some() {
            self.ui.surface_drag = None;
            changed = true;
        }
        if changed {
            self.bump_local_revision();
        }
        self.app_state.snapshot_model().active_workspace_id()
    }

    fn ensure_active_window_visible(&mut self) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let viewport_frame = self.workspace_viewport_frame(attention_panel_visible(&model));
        let current_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            workspace.viewport.clone(),
        );
        let Some(active_frame) =
            workspace_window_placements(workspace, viewport_frame.width, viewport_frame.height)
                .into_iter()
                .find(|placement| placement.window_id == workspace.active_window)
                .map(|placement| placement.frame)
        else {
            return false;
        };

        let mut next_viewport = current_viewport;
        let visible_right = next_viewport.x + viewport_frame.width;
        let visible_bottom = next_viewport.y + viewport_frame.height;
        if active_frame.x < next_viewport.x {
            next_viewport.x = active_frame.x;
        } else if active_frame.right() > visible_right {
            next_viewport.x = active_frame.right() - viewport_frame.width;
        }
        if active_frame.y < next_viewport.y {
            next_viewport.y = active_frame.y;
        } else if active_frame.bottom() > visible_bottom {
            next_viewport.y = active_frame.bottom() - viewport_frame.height;
        }
        let next_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            next_viewport,
        );
        if next_viewport == workspace.viewport {
            return false;
        }
        self.dispatch_control(ControlCommand::SetWorkspaceViewport {
            workspace_id,
            viewport: next_viewport,
        })
    }

    fn with_active_browser_surface(
        &mut self,
        handler: impl FnOnce(&mut Self, SurfaceId) -> bool,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let Some(pane) = workspace.panes.get(&workspace.active_pane) else {
            return false;
        };
        let Some(surface) = pane.active_surface() else {
            return false;
        };
        if surface.kind != PaneKind::Browser {
            return false;
        }
        handler(self, surface.id)
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
        model
            .workspaces
            .iter()
            .find_map(|(workspace_id, workspace)| {
                workspace
                    .panes
                    .contains_key(&pane_id)
                    .then_some((*workspace_id, pane_id))
            })
    }

    fn resolve_surface_location(
        &self,
        model: &AppModel,
        surface_id: SurfaceId,
    ) -> Option<(WorkspaceId, PaneId)> {
        model
            .workspaces
            .iter()
            .find_map(|(workspace_id, workspace)| {
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
                let _ = self.sync_revision_from_app();
                Some(response)
            }
            Err(error) => {
                eprintln!("taskers control dispatch failed: {error}");
                None
            }
        }
    }

    fn sync_revision_from_app(&mut self) -> bool {
        let app_revision = self.app_state.revision();
        if self.observed_app_revision == app_revision {
            return false;
        }
        self.observed_app_revision = app_revision;
        self.revision = self.revision.saturating_add(1).max(app_revision);
        true
    }

    fn bump_local_revision(&mut self) {
        self.revision = self
            .revision
            .max(self.observed_app_revision)
            .saturating_add(1);
    }

    fn drain_host_commands(&mut self) -> Vec<HostCommand> {
        self.host_commands.drain(..).collect()
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

    pub fn selected_shortcut_preset(&self) -> ShortcutPreset {
        self.inner.lock().ui.selected_shortcut_preset
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

    pub fn dispatch_shortcut_action(&self, action: ShortcutAction) -> bool {
        let mut inner = self.inner.lock();
        let changed = inner.dispatch_shortcut_action(action);
        if changed {
            let _ = self.revisions.send(inner.revision());
        }
        changed
    }

    pub fn apply_host_event(&self, event: HostEvent) {
        let mut inner = self.inner.lock();
        if inner.apply_host_event(event) {
            let _ = self.revisions.send(inner.revision());
        }
    }

    pub fn sync_external_changes(&self) {
        let mut inner = self.inner.lock();
        if inner.sync_revision_from_app() {
            let _ = self.revisions.send(inner.revision());
        }
    }

    pub fn drain_host_commands(&self) -> Vec<HostCommand> {
        self.inner.lock().drain_host_commands()
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

fn shortcut_bindings(preset: ShortcutPreset) -> Vec<ShortcutBindingSnapshot> {
    ShortcutAction::ALL
        .into_iter()
        .map(|action| ShortcutBindingSnapshot {
            id: action.id().into(),
            label: action.label().into(),
            detail: action.detail().into(),
            category: action.category().into(),
            accelerators: action
                .accelerators(preset)
                .iter()
                .map(|value| (*value).into())
                .collect(),
        })
        .collect()
}

fn display_window_frame(
    frame: WindowFrame,
    metrics: CanvasMetrics,
    viewport: Frame,
    viewport_x: i32,
    viewport_y: i32,
    overview_mode: bool,
) -> Frame {
    let translated_x = viewport.x + metrics.offset_x + frame.x;
    let translated_y = viewport.y + metrics.offset_y + frame.y;
    let shift_x = if overview_mode { 0 } else { viewport_x };
    let shift_y = if overview_mode { 0 } else { viewport_y };
    Frame::new(
        translated_x - shift_x,
        translated_y - shift_y,
        frame.width,
        frame.height,
    )
}

fn workspace_render_context(
    workspace: &Workspace,
    overview_mode: bool,
    viewport_width: i32,
    viewport_height: i32,
    metrics: LayoutMetrics,
) -> WorkspaceRenderContext {
    if !overview_mode {
        return WorkspaceRenderContext {
            overview_mode: false,
            overview_scale: 1.0,
            outer_padding: 0,
            viewport_width,
            viewport_height,
        };
    }

    let base_frames = workspace_window_placements(workspace, viewport_width, viewport_height)
        .into_iter()
        .map(|placement| placement.frame)
        .collect::<Vec<_>>();
    let base_metrics = canvas_metrics_from_frames(&base_frames, 0);
    let outer_padding = metrics.workspace_padding;
    let available_width = (viewport_width - outer_padding * 2).max(1);
    let available_height = (viewport_height - outer_padding * 2).max(1);
    let overview_scale = (f64::from(available_width) / f64::from(base_metrics.width.max(1)))
        .min(f64::from(available_height) / f64::from(base_metrics.height.max(1)))
        .clamp(0.05, 1.0);

    WorkspaceRenderContext {
        overview_mode: true,
        overview_scale,
        outer_padding,
        viewport_width,
        viewport_height,
    }
}

fn workspace_display_window_placements(
    workspace: &Workspace,
    render_context: WorkspaceRenderContext,
) -> Vec<WorkspaceWindowPlacement> {
    workspace_window_placements(
        workspace,
        render_context.viewport_width,
        render_context.viewport_height,
    )
    .into_iter()
    .map(|mut placement| {
        if render_context.overview_mode {
            placement.frame = scale_window_frame(placement.frame, render_context.overview_scale);
        }
        placement
    })
    .collect()
}

fn workspace_window_placements(
    workspace: &Workspace,
    viewport_width: i32,
    viewport_height: i32,
) -> Vec<WorkspaceWindowPlacement> {
    let ordered_columns = workspace.columns.values().collect::<Vec<_>>();
    if ordered_columns.is_empty() {
        return Vec::new();
    }

    let horizontal_gap_total =
        DEFAULT_WORKSPACE_WINDOW_GAP * ordered_columns.len().saturating_sub(1) as i32;
    let available_width = (viewport_width - horizontal_gap_total).max(0);
    let preferred_column_widths = ordered_columns
        .iter()
        .map(|column| column.width.max(1))
        .collect::<Vec<_>>();
    let column_widths = fit_track_extents(
        &preferred_column_widths,
        available_width,
        MIN_WORKSPACE_WINDOW_WIDTH,
    );

    let mut placements = Vec::new();
    let mut x = 0;
    for (column_index, column) in ordered_columns.into_iter().enumerate() {
        let column_width = column_widths
            .get(column_index)
            .copied()
            .unwrap_or(MIN_WORKSPACE_WINDOW_WIDTH);
        let vertical_gap_total =
            DEFAULT_WORKSPACE_WINDOW_GAP * column.window_order.len().saturating_sub(1) as i32;
        let available_height = (viewport_height - vertical_gap_total).max(0);
        let preferred_window_heights = column
            .window_order
            .iter()
            .filter_map(|window_id| workspace.windows.get(window_id).map(|window| window.height))
            .collect::<Vec<_>>();
        let window_heights = fit_track_extents(
            &preferred_window_heights,
            available_height,
            MIN_WORKSPACE_WINDOW_HEIGHT,
        );

        let mut y = 0;
        for (window_index, window_id) in column.window_order.iter().enumerate() {
            if !workspace.windows.contains_key(window_id) {
                continue;
            }
            let window_height = window_heights
                .get(window_index)
                .copied()
                .unwrap_or(MIN_WORKSPACE_WINDOW_HEIGHT);
            placements.push(WorkspaceWindowPlacement {
                window_id: *window_id,
                column_id: column.id,
                frame: WindowFrame {
                    x,
                    y,
                    width: column_width,
                    height: window_height,
                },
            });
            y += window_height + DEFAULT_WORKSPACE_WINDOW_GAP;
        }

        x += column_width + DEFAULT_WORKSPACE_WINDOW_GAP;
    }

    placements
}

fn workspace_canvas_metrics(
    placements: &[WorkspaceWindowPlacement],
    outer_padding: i32,
) -> CanvasMetrics {
    let frames = placements
        .iter()
        .map(|placement| placement.frame)
        .collect::<Vec<_>>();
    canvas_metrics_from_frames(&frames, outer_padding)
}

fn clamped_workspace_viewport(
    workspace: &Workspace,
    viewport_width: i32,
    viewport_height: i32,
    viewport: taskers_domain::WorkspaceViewport,
) -> taskers_domain::WorkspaceViewport {
    let placements = workspace_window_placements(workspace, viewport_width, viewport_height);
    let canvas = workspace_canvas_metrics(&placements, 0);
    let max_x = (canvas.width - viewport_width).max(0);
    let max_y = (canvas.height - viewport_height).max(0);

    taskers_domain::WorkspaceViewport {
        x: viewport.x.clamp(0, max_x),
        y: viewport.y.clamp(0, max_y),
    }
}

fn canvas_metrics_from_frames(frames: &[WindowFrame], outer_padding: i32) -> CanvasMetrics {
    let min_x = frames.iter().map(|frame| frame.x).min().unwrap_or(0);
    let min_y = frames.iter().map(|frame| frame.y).min().unwrap_or(0);
    let offset_x = outer_padding - min_x;
    let offset_y = outer_padding - min_y;
    let width = frames
        .iter()
        .map(|frame| frame.right() + offset_x + outer_padding)
        .max()
        .unwrap_or(outer_padding.saturating_mul(2));
    let height = frames
        .iter()
        .map(|frame| frame.bottom() + offset_y + outer_padding)
        .max()
        .unwrap_or(outer_padding.saturating_mul(2));

    CanvasMetrics {
        offset_x,
        offset_y,
        width,
        height,
    }
}

fn scale_window_frame(frame: WindowFrame, scale: f64) -> WindowFrame {
    WindowFrame {
        x: (f64::from(frame.x) * scale).round() as i32,
        y: (f64::from(frame.y) * scale).round() as i32,
        width: (f64::from(frame.width) * scale).round() as i32,
        height: (f64::from(frame.height) * scale).round() as i32,
    }
}

fn fit_track_extents(preferred_extents: &[i32], available_total: i32, min_extent: i32) -> Vec<i32> {
    if preferred_extents.is_empty() {
        return Vec::new();
    }

    let count = preferred_extents.len() as i32;
    let min_total = min_extent.saturating_mul(count);
    if available_total <= min_total {
        return vec![min_extent; preferred_extents.len()];
    }

    let preferred_extents = preferred_extents
        .iter()
        .map(|extent| (*extent).max(1))
        .collect::<Vec<_>>();
    let mut result = vec![0; preferred_extents.len()];
    let mut active = (0..preferred_extents.len()).collect::<Vec<_>>();
    let mut remaining_total = available_total;

    loop {
        if active.is_empty() {
            break;
        }

        let remaining_weight = active
            .iter()
            .map(|index| i64::from(preferred_extents[*index]))
            .sum::<i64>()
            .max(1);
        let below_minimum = active
            .iter()
            .copied()
            .filter(|index| {
                (f64::from(remaining_total) * f64::from(preferred_extents[*index]))
                    / (remaining_weight as f64)
                    < f64::from(min_extent)
            })
            .collect::<Vec<_>>();

        if below_minimum.is_empty() {
            let distributed = distribute_weighted_total(
                &active
                    .iter()
                    .map(|index| preferred_extents[*index])
                    .collect::<Vec<_>>(),
                remaining_total,
            );
            for (slot, index) in active.iter().enumerate() {
                result[*index] = distributed[slot];
            }
            break;
        }

        for index in below_minimum {
            result[index] = min_extent;
            remaining_total -= min_extent;
            active.retain(|candidate| *candidate != index);
        }
    }

    result
}

fn distribute_weighted_total(weights: &[i32], total: i32) -> Vec<i32> {
    if weights.is_empty() {
        return Vec::new();
    }
    let weight_sum = weights
        .iter()
        .map(|weight| i64::from(*weight))
        .sum::<i64>()
        .max(1);
    let mut distributed = Vec::with_capacity(weights.len());
    let mut allocated = 0;
    let mut remainders = Vec::with_capacity(weights.len());

    for (index, weight) in weights.iter().copied().enumerate() {
        let scaled = i64::from(total) * i64::from(weight);
        let base = (scaled / weight_sum) as i32;
        distributed.push(base);
        allocated += base;
        remainders.push((index, scaled % weight_sum));
    }

    let mut remaining = total - allocated;
    remainders.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    for (index, _) in remainders.into_iter().take(remaining.max(0) as usize) {
        distributed[index] += 1;
        remaining -= 1;
        if remaining <= 0 {
            break;
        }
    }

    distributed
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

    if let Some(workspace) = model.workspaces.get_mut(&workspace_id) {
        workspace.custom_color = Some("#bb9af7".into());
    }

    AppState::new(
        model,
        default_session_path_for_preview("taskers-preview-bootstrap"),
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

fn pane_body_frame(
    frame: Frame,
    metrics: LayoutMetrics,
    kind: &PaneKind,
    show_tab_strip: bool,
) -> Frame {
    let browser_toolbar_height = match kind {
        PaneKind::Terminal => 0,
        PaneKind::Browser => metrics.browser_toolbar_height,
    };
    let tab_strip_height = if show_tab_strip {
        metrics.surface_tab_height
    } else {
        0
    };
    frame
        .inset(metrics.pane_border_width)
        .inset_top(metrics.pane_header_height + tab_strip_height + browser_toolbar_height)
}

fn workspace_window_content_frame(frame: Frame, metrics: LayoutMetrics) -> Frame {
    frame
        .inset(metrics.window_border_width)
        .inset_top(metrics.window_toolbar_height)
        .inset(metrics.window_body_padding)
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

fn workspace_window_attention(
    workspace: &Workspace,
    window: &taskers_domain::WorkspaceWindowRecord,
) -> AttentionState {
    window
        .layout
        .leaves()
        .into_iter()
        .filter_map(|pane_id| workspace.panes.get(&pane_id))
        .map(|pane| pane.highest_attention())
        .max_by_key(|attention| attention.rank())
        .unwrap_or(taskers_domain::AttentionState::Normal)
        .into()
}

fn surface_runtime_identity(
    surface: &SurfaceRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    let key = runtime_key(surface);
    RuntimeIdentitySnapshot {
        label: runtime_label(&key),
        state: surface_runtime_state(surface, now),
        key,
    }
}

fn pane_runtime_identity(
    pane: &taskers_domain::PaneRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    pane.active_surface()
        .map(|surface| surface_runtime_identity(surface, now))
        .unwrap_or_else(|| fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle))
}

fn workspace_window_runtime_identity(
    workspace: &Workspace,
    window: &taskers_domain::WorkspaceWindowRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    dominant_runtime_identity(
        window
            .layout
            .leaves()
            .into_iter()
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .map(|pane| {
                (
                    pane_runtime_identity(pane, now),
                    pane.id == window.active_pane,
                )
            }),
        fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle),
    )
}

fn workspace_runtime_identity(
    workspace: Option<&Workspace>,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    workspace
        .map(|workspace| {
            dominant_runtime_identity(
                workspace.windows.values().map(|window| {
                    (
                        workspace_window_runtime_identity(workspace, window, now),
                        window.id == workspace.active_window,
                    )
                }),
                fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle),
            )
        })
        .unwrap_or_else(|| fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle))
}

fn fallback_runtime_identity(key: &str, state: RuntimeStateSnapshot) -> RuntimeIdentitySnapshot {
    RuntimeIdentitySnapshot {
        key: key.to_string(),
        label: runtime_label(key),
        state,
    }
}

fn dominant_runtime_identity<I>(
    candidates: I,
    fallback: RuntimeIdentitySnapshot,
) -> RuntimeIdentitySnapshot
where
    I: IntoIterator<Item = (RuntimeIdentitySnapshot, bool)>,
{
    let mut best: Option<(RuntimeIdentitySnapshot, bool)> = None;
    for candidate in candidates {
        let replace = best.as_ref().is_none_or(|(best_runtime, best_active)| {
            runtime_priority(&candidate.0, candidate.1)
                < runtime_priority(best_runtime, *best_active)
        });
        if replace {
            best = Some(candidate);
        }
    }
    best.map(|(runtime, _)| runtime).unwrap_or(fallback)
}

fn runtime_priority(runtime: &RuntimeIdentitySnapshot, active: bool) -> (u8, u8, u8) {
    (
        runtime_state_priority(runtime.state),
        if active { 0 } else { 1 },
        runtime_kind_priority(&runtime.key),
    )
}

fn runtime_state_priority(state: RuntimeStateSnapshot) -> u8 {
    match state {
        RuntimeStateSnapshot::Failed => 0,
        RuntimeStateSnapshot::Waiting => 1,
        RuntimeStateSnapshot::Working => 2,
        RuntimeStateSnapshot::Completed => 3,
        RuntimeStateSnapshot::Idle => 4,
    }
}

fn runtime_kind_priority(key: &str) -> u8 {
    match key {
        "browser" => 1,
        "terminal" => 2,
        _ => 0,
    }
}

fn dominant_attention_ring(
    rings: impl IntoIterator<Item = AttentionRingState>,
) -> Option<AttentionRingState> {
    rings.into_iter().min_by_key(|ring| match ring {
        AttentionRingState::Error => 0,
        AttentionRingState::Waiting => 1,
        AttentionRingState::Completed => 2,
    })
}

fn runtime_key(surface: &SurfaceRecord) -> String {
    if let Some(agent_kind) = surface
        .metadata
        .agent_kind
        .as_deref()
        .map(str::trim)
        .filter(|agent_kind| !agent_kind.is_empty() && *agent_kind != "shell")
    {
        return agent_kind.to_ascii_lowercase();
    }

    match surface.kind {
        PaneKind::Browser => "browser".into(),
        PaneKind::Terminal => "terminal".into(),
    }
}

fn runtime_label(key: &str) -> String {
    match key {
        "codex" => "Codex".into(),
        "claude" => "Claude".into(),
        "opencode" => "OpenCode".into(),
        "aider" => "Aider".into(),
        "browser" => "Browser".into(),
        "terminal" => "Terminal".into(),
        other => other
            .split(|ch: char| matches!(ch, '-' | '_' | ' '))
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                let Some(first) = chars.next() else {
                    return String::new();
                };
                let mut label = String::new();
                label.push(first.to_ascii_uppercase());
                label.push_str(chars.as_str());
                label
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn surface_runtime_state(surface: &SurfaceRecord, now: OffsetDateTime) -> RuntimeStateSnapshot {
    surface_agent_state(surface, now)
        .map(runtime_state_from_agent_state)
        .unwrap_or(RuntimeStateSnapshot::Idle)
}

fn surface_agent_state(
    surface: &SurfaceRecord,
    now: OffsetDateTime,
) -> Option<taskers_domain::WorkspaceAgentState> {
    let agent_kind = surface
        .metadata
        .agent_kind
        .as_deref()
        .map(str::trim)
        .filter(|agent_kind| !agent_kind.is_empty() && *agent_kind != "shell")?;
    let _ = agent_kind;

    let state = surface.metadata.agent_state.or_else(|| {
        if surface.metadata.agent_active {
            match surface.attention {
                taskers_domain::AttentionState::Busy => {
                    Some(taskers_domain::WorkspaceAgentState::Working)
                }
                taskers_domain::AttentionState::WaitingInput => {
                    Some(taskers_domain::WorkspaceAgentState::Waiting)
                }
                taskers_domain::AttentionState::Completed => {
                    Some(taskers_domain::WorkspaceAgentState::Completed)
                }
                taskers_domain::AttentionState::Error => {
                    Some(taskers_domain::WorkspaceAgentState::Failed)
                }
                taskers_domain::AttentionState::Normal => None,
            }
        } else {
            match surface.attention {
                taskers_domain::AttentionState::Completed => {
                    Some(taskers_domain::WorkspaceAgentState::Completed)
                }
                taskers_domain::AttentionState::Error => {
                    Some(taskers_domain::WorkspaceAgentState::Failed)
                }
                taskers_domain::AttentionState::Busy
                | taskers_domain::AttentionState::WaitingInput => {
                    Some(taskers_domain::WorkspaceAgentState::Completed)
                }
                taskers_domain::AttentionState::Normal => None,
            }
        }
    })?;

    match state {
        taskers_domain::WorkspaceAgentState::Working
        | taskers_domain::WorkspaceAgentState::Waiting => Some(state),
        taskers_domain::WorkspaceAgentState::Completed
        | taskers_domain::WorkspaceAgentState::Failed => surface
            .metadata
            .last_signal_at
            .filter(|timestamp| *timestamp >= now - time::Duration::minutes(15))
            .map(|_| state),
    }
}

fn runtime_state_from_agent_state(
    state: taskers_domain::WorkspaceAgentState,
) -> RuntimeStateSnapshot {
    match state {
        taskers_domain::WorkspaceAgentState::Working => RuntimeStateSnapshot::Working,
        taskers_domain::WorkspaceAgentState::Waiting => RuntimeStateSnapshot::Waiting,
        taskers_domain::WorkspaceAgentState::Completed => RuntimeStateSnapshot::Completed,
        taskers_domain::WorkspaceAgentState::Failed => RuntimeStateSnapshot::Failed,
    }
}

fn window_primary_title(
    workspace: &Workspace,
    window: &taskers_domain::WorkspaceWindowRecord,
) -> String {
    workspace
        .panes
        .get(&window.active_pane)
        .and_then(|pane| pane.active_surface())
        .map(display_surface_title)
        .unwrap_or_else(|| "Workspace window".into())
}

fn display_surface_title(surface: &SurfaceRecord) -> String {
    match surface.kind {
        PaneKind::Terminal => display_terminal_title(&surface.metadata),
        PaneKind::Browser => display_browser_title(&surface.metadata),
    }
}

fn surface_activity_label(surface: &SurfaceRecord, now: OffsetDateTime) -> Option<String> {
    let _ = active_agent_surface_state(surface, now)?;
    surface
        .metadata
        .latest_agent_message
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
}

fn surface_status_label(surface: &SurfaceRecord, now: OffsetDateTime) -> Option<String> {
    match active_agent_surface_state(surface, now)? {
        RuntimeStateSnapshot::Waiting => Some("Awaiting response".into()),
        RuntimeStateSnapshot::Working => Some("Working".into()),
        RuntimeStateSnapshot::Completed => Some("Completed".into()),
        RuntimeStateSnapshot::Failed => Some("Failed".into()),
        RuntimeStateSnapshot::Idle => None,
    }
}

fn active_agent_surface_state(
    surface: &SurfaceRecord,
    now: OffsetDateTime,
) -> Option<RuntimeStateSnapshot> {
    if surface.kind != PaneKind::Terminal {
        return None;
    }

    let state = surface_runtime_state(surface, now);
    (!matches!(state, RuntimeStateSnapshot::Idle)).then_some(state)
}

fn surface_notification_ring(surface: &SurfaceRecord) -> Option<AttentionRingState> {
    if surface.kind != PaneKind::Terminal {
        return None;
    }

    let is_agent_surface = surface
        .metadata
        .agent_kind
        .as_deref()
        .map(str::trim)
        .filter(|agent_kind| !agent_kind.is_empty() && *agent_kind != "shell")
        .is_some();
    if !is_agent_surface {
        return None;
    }

    match surface.attention {
        taskers_domain::AttentionState::WaitingInput => Some(AttentionRingState::Waiting),
        taskers_domain::AttentionState::Error => Some(AttentionRingState::Error),
        taskers_domain::AttentionState::Completed => Some(AttentionRingState::Completed),
        taskers_domain::AttentionState::Normal | taskers_domain::AttentionState::Busy => None,
    }
}

fn display_terminal_title(metadata: &PaneMetadata) -> String {
    let agent_title = metadata
        .agent_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let context = terminal_context_label(metadata);

    if let Some(agent_title) = agent_title {
        if let Some(context) = context.as_deref() {
            return format!("{agent_title} · {context}");
        }
        return agent_title.to_string();
    }

    if let Some(context) = context {
        return context;
    }

    if let Some(title) = metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .filter(|title| !is_generic_terminal_title(title))
    {
        return title.to_string();
    }

    "Terminal".into()
}

fn display_browser_title(metadata: &PaneMetadata) -> String {
    if let Some(title) = metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return title.to_string();
    }

    if let Some(url) = metadata
        .url
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        return url.to_string();
    }

    "Browser".into()
}

fn terminal_context_label(metadata: &PaneMetadata) -> Option<String> {
    let repo_name = metadata
        .repo_name
        .as_deref()
        .map(str::trim)
        .filter(|repo| !repo.is_empty());
    let git_branch = metadata
        .git_branch
        .as_deref()
        .map(str::trim)
        .filter(|branch| !branch.is_empty());

    if let Some(repo_name) = repo_name {
        return Some(match git_branch {
            Some(git_branch) => format!("{repo_name}/{git_branch}"),
            None => repo_name.to_string(),
        });
    }

    normalized_cwd(metadata)
        .as_deref()
        .and_then(path_basename)
        .map(str::to_string)
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

fn path_basename(path: &str) -> Option<&str> {
    let trimmed = path.trim().trim_end_matches(|ch| matches!(ch, '/' | '\\'));
    if trimmed.is_empty() {
        return None;
    }

    trimmed
        .rsplit(|ch| matches!(ch, '/' | '\\'))
        .find(|segment| !segment.is_empty())
}

fn is_generic_terminal_title(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }

    let mut parts = trimmed.split_whitespace();
    let command = parts.next().unwrap_or_default();
    if !parts.all(|part| part.starts_with('-')) {
        return false;
    }

    let basename = command
        .rsplit(|ch| matches!(ch, '/' | '\\'))
        .next()
        .unwrap_or(command)
        .trim()
        .to_ascii_lowercase();

    matches!(
        basename.as_str(),
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "nu"
            | "nushell"
            | "dash"
            | "ash"
            | "ksh"
            | "mksh"
            | "pwsh"
            | "powershell"
            | "cmd"
            | "cmd.exe"
            | "xonsh"
            | "elvish"
    )
}

fn pane_shows_tab_strip_for_surface_count(surface_count: usize) -> bool {
    surface_count > 1
}

fn format_relative_time(timestamp: OffsetDateTime) -> String {
    let now = OffsetDateTime::now_utc();
    let delta = now - timestamp;
    let seconds = delta.whole_seconds();
    if seconds < 0 {
        return "just now".into();
    }
    if seconds < 60 {
        return "just now".into();
    }
    let minutes = delta.whole_minutes();
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = delta.whole_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = delta.whole_days();
    if days < 7 {
        return format!("{days}d ago");
    }
    format!("{days}d ago")
}

fn compact_preview(message: &str) -> String {
    let trimmed = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.len() <= 140 {
        return trimmed;
    }
    format!("{}…", &trimmed[..139])
}

fn activity_title(model: &AppModel, item: &ActivityItem) -> String {
    if let Some(title) = item
        .title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        return title.to_string();
    }

    model
        .workspaces
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

fn activity_item_snapshot(model: &AppModel, item: &ActivityItem) -> ActivityItemSnapshot {
    let title = activity_title(model, item);
    let body = if item.message.trim() != title.trim() && !item.message.is_empty() {
        Some(item.message.clone())
    } else {
        None
    };
    let source_workspace_title = model
        .workspaces
        .get(&item.workspace_id)
        .map(|workspace| workspace.label.clone());
    ActivityItemSnapshot {
        id: ActivityId {
            notification_id: item.notification_id,
        },
        title,
        preview: compact_preview(&item.message),
        meta: activity_context_line(model, item),
        attention: item.state.into(),
        workspace_id: item.workspace_id,
        pane_id: Some(item.pane_id),
        surface_id: Some(item.surface_id),
        unread: item.read_at.is_none(),
        timestamp: format_relative_time(item.created_at),
        body,
        source_workspace_title,
    }
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

fn workspace_progress_snapshot(workspace: &Workspace) -> Option<ProgressSnapshot> {
    workspace
        .progress
        .as_ref()
        .map(|progress| ProgressSnapshot {
            fraction: f32::from(progress.value.min(1000)) / 1000.0,
            label: progress.label.clone(),
        })
        .or_else(|| {
            workspace
                .panes
                .values()
                .flat_map(|pane| pane.surfaces.values())
                .find_map(|surface| {
                    surface
                        .metadata
                        .progress
                        .as_ref()
                        .map(|progress| ProgressSnapshot {
                            fraction: f32::from(progress.value.min(1000)) / 1000.0,
                            label: progress.label.clone(),
                        })
                })
        })
}

fn attention_panel_visible(model: &AppModel) -> bool {
    model
        .workspace_summaries(model.active_window)
        .map(|summaries| {
            summaries
                .iter()
                .any(|summary| !summary.agent_summaries.is_empty() || summary.status_text.is_some())
        })
        .unwrap_or(false)
        || model.workspaces.values().any(|workspace| {
            !workspace.notifications.is_empty()
                || !workspace.log_entries.is_empty()
                || workspace.progress.is_some()
        })
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
                .unwrap_or_else(|| DEFAULT_BROWSER_HOME.into()),
        }),
        PaneKind::Terminal => SurfaceMountSpec::Terminal(TerminalMountSpec {
            title: display_surface_title(surface),
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
    if is_local_browser_target(trimmed) {
        return format!("http://{trimmed}");
    }
    if has_explicit_uri_scheme(trimmed) {
        return trimmed.to_string();
    }
    if !looks_like_browser_location(trimmed) {
        return format!(
            "https://duckduckgo.com/?q={}",
            trimmed.split_whitespace().collect::<Vec<_>>().join("+")
        );
    }
    format!("https://{trimmed}")
}

fn has_explicit_uri_scheme(value: &str) -> bool {
    let Some((scheme, _)) = value.split_once(':') else {
        return false;
    };
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        && scheme
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn looks_like_browser_location(value: &str) -> bool {
    if is_local_browser_target(value) || value.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }

    let head = value.split(['/', '?', '#']).next().unwrap_or(value);
    head.contains('.')
}

fn is_local_browser_target(value: &str) -> bool {
    value.starts_with("localhost")
        || value.starts_with("127.0.0.1")
        || value.starts_with("192.168.")
        || value.starts_with("10.")
        || value.starts_with("0.0.0.0")
        || value.contains(":3000")
        || value.contains(":5173")
        || value.contains(":8000")
        || value.contains(":8080")
}

#[cfg(test)]
mod tests {
    use taskers_control::ControlCommand;
    use taskers_core::AppState;
    use taskers_domain::{
        AppModel, AttentionState as DomainAttentionState, NotificationId, NotificationItem,
        SignalKind,
    };
    use taskers_ghostty::BackendChoice;
    use taskers_runtime::ShellLaunchSpec;
    use time::OffsetDateTime;

    use super::{
        BootstrapModel, BrowserMountSpec, DEFAULT_BROWSER_HOME, Direction, HostCommand, HostEvent,
        LayoutMetrics, NotificationPreferencesSnapshot, RuntimeCapability, RuntimeStatus,
        SharedCore, ShellAction, ShellDragMode, ShellSection, SurfaceDragSessionSnapshot,
        SurfaceMountSpec, WorkspaceDirection, default_preview_app_state,
        default_session_path_for_preview, display_surface_title, pane_body_frame,
        pane_shows_tab_strip_for_surface_count, resolved_browser_uri, split_frame,
        workspace_window_content_frame,
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
            notification_preferences: NotificationPreferencesSnapshot::default(),
        }
    }

    fn bootstrap_with_notification(cleared: bool) -> BootstrapModel {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let (pane_id, surface_id) = {
            let workspace = model.workspaces.get(&workspace_id).expect("workspace");
            let pane_id = workspace.active_pane;
            let surface_id = workspace
                .panes
                .get(&pane_id)
                .and_then(|pane| pane.active_surface())
                .map(|surface| surface.id)
                .expect("surface");
            (pane_id, surface_id)
        };
        let now = OffsetDateTime::now_utc();
        model
            .workspaces
            .get_mut(&workspace_id)
            .expect("workspace")
            .notifications
            .push(NotificationItem {
                id: NotificationId::new(),
                pane_id,
                surface_id,
                kind: SignalKind::Notification,
                state: DomainAttentionState::WaitingInput,
                title: Some("Heads up".into()),
                subtitle: None,
                external_id: None,
                message: "Needs attention".into(),
                created_at: now,
                read_at: cleared.then_some(now),
                cleared_at: cleared.then_some(now),
                desktop_delivery: taskers_domain::NotificationDeliveryState::Shown,
            });

        BootstrapModel {
            app_state: AppState::new(
                model,
                default_session_path_for_preview(if cleared {
                    "taskers-preview-done-activity"
                } else {
                    "taskers-preview-activity"
                }),
                BackendChoice::Mock,
                ShellLaunchSpec::fallback(),
            )
            .expect("preview app state"),
            ..bootstrap()
        }
    }

    fn bootstrap_with_model(model: AppModel, name: &str) -> BootstrapModel {
        BootstrapModel {
            app_state: super::AppState::new(
                model,
                default_session_path_for_preview(name),
                super::BackendChoice::Mock,
                super::ShellLaunchSpec::fallback(),
            )
            .expect("preview app state"),
            ..bootstrap()
        }
    }

    fn find_pane<'a>(
        node: &'a super::LayoutNodeSnapshot,
        pane_id: taskers_domain::PaneId,
    ) -> Option<&'a super::PaneSnapshot> {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => (pane.id == pane_id).then_some(pane),
            super::LayoutNodeSnapshot::Split { first, second, .. } => {
                find_pane(first, pane_id).or_else(|| find_pane(second, pane_id))
            }
        }
    }

    fn find_pane_frame(
        node: &super::LayoutNodeSnapshot,
        pane_id: taskers_domain::PaneId,
        frame: super::Frame,
        gap: i32,
    ) -> Option<super::Frame> {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => (pane.id == pane_id).then_some(frame),
            super::LayoutNodeSnapshot::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = (ratio.clamp(0.15, 0.85) * 1000.0).round() as u16;
                let (first_frame, second_frame) = split_frame(frame, *axis, ratio, gap);
                find_pane_frame(first, pane_id, first_frame, gap)
                    .or_else(|| find_pane_frame(second, pane_id, second_frame, gap))
            }
        }
    }

    fn collect_pane_ids(
        node: &super::LayoutNodeSnapshot,
        pane_ids: &mut Vec<taskers_domain::PaneId>,
    ) {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => pane_ids.push(pane.id),
            super::LayoutNodeSnapshot::Split { first, second, .. } => {
                collect_pane_ids(first, pane_ids);
                collect_pane_ids(second, pane_ids);
            }
        }
    }

    fn surface_with_metadata(
        kind: taskers_domain::PaneKind,
        metadata: taskers_domain::PaneMetadata,
    ) -> taskers_domain::SurfaceRecord {
        let mut surface = taskers_domain::SurfaceRecord::new(kind);
        surface.metadata = metadata;
        surface
    }

    #[test]
    fn terminal_surface_titles_prefer_agent_and_repo_context() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("/usr/bin/zsh".into()),
                agent_title: Some("Codex".into()),
                repo_name: Some("taskers".into()),
                git_branch: Some("main".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(display_surface_title(&surface), "Codex · taskers/main");
    }

    #[test]
    fn terminal_surface_titles_prefer_repo_context_over_generic_shell_names() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("/usr/bin/zsh".into()),
                repo_name: Some("taskers".into()),
                git_branch: Some("main".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(display_surface_title(&surface), "taskers/main");
    }

    #[test]
    fn terminal_surface_titles_fall_back_to_cwd_basename_before_generic_shell_names() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("zsh".into()),
                cwd: Some("/home/notes/Projects/taskers".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(display_surface_title(&surface), "taskers");
    }

    #[test]
    fn terminal_surface_titles_keep_non_generic_host_titles_as_fallback() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("OpenAI Codex".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(display_surface_title(&surface), "OpenAI Codex");
    }

    #[test]
    fn browser_surface_titles_stay_page_title_first() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Browser,
            taskers_domain::PaneMetadata {
                title: Some("Taskers Docs".into()),
                url: Some("https://example.com/docs".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(display_surface_title(&surface), "Taskers Docs");
    }

    #[test]
    fn surface_runtime_identity_prefers_agent_key_and_recent_state() {
        let now = OffsetDateTime::now_utc();
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: true,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Waiting),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        let runtime = super::surface_runtime_identity(&surface, now);
        assert_eq!(runtime.key, "codex");
        assert_eq!(runtime.label, "Codex");
        assert_eq!(runtime.state, super::RuntimeStateSnapshot::Waiting);
    }

    #[test]
    fn waiting_agent_labels_prefer_latest_message_and_human_status() {
        let now = OffsetDateTime::now_utc();
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: true,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Waiting),
                latest_agent_message: Some("Summarize recent commits".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(
            super::surface_activity_label(&surface, now).as_deref(),
            Some("Summarize recent commits")
        );
        assert_eq!(
            super::surface_status_label(&surface, now).as_deref(),
            Some("Awaiting response")
        );
    }

    #[test]
    fn working_agent_labels_keep_activity_message_and_status() {
        let now = OffsetDateTime::now_utc();
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: true,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Working),
                latest_agent_message: Some("Updating tests".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(
            super::surface_activity_label(&surface, now).as_deref(),
            Some("Updating tests")
        );
        assert_eq!(
            super::surface_status_label(&surface, now).as_deref(),
            Some("Working")
        );
    }

    #[test]
    fn recent_completed_and_failed_agents_keep_status_badges() {
        let now = OffsetDateTime::now_utc();
        let completed = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: false,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Completed),
                latest_agent_message: Some("Finished sync".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        let failed = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: false,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Failed),
                latest_agent_message: Some("Migration failed".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(
            super::surface_activity_label(&completed, now).as_deref(),
            Some("Finished sync")
        );
        assert_eq!(
            super::surface_status_label(&completed, now).as_deref(),
            Some("Completed")
        );
        assert_eq!(
            super::surface_activity_label(&failed, now).as_deref(),
            Some("Migration failed")
        );
        assert_eq!(
            super::surface_status_label(&failed, now).as_deref(),
            Some("Failed")
        );
    }

    #[test]
    fn stale_terminal_agents_clear_activity_and_status_labels() {
        let now = OffsetDateTime::now_utc();
        let stale_timestamp = now - time::Duration::minutes(16);
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: false,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Completed),
                latest_agent_message: Some("Finished sync".into()),
                last_signal_at: Some(stale_timestamp),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(super::surface_activity_label(&surface, now), None);
        assert_eq!(super::surface_status_label(&surface, now), None);
    }

    #[test]
    fn non_agent_and_browser_surfaces_do_not_emit_activity_or_status_labels() {
        let now = OffsetDateTime::now_utc();
        let terminal = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("zsh".into()),
                latest_agent_message: Some("Ignored".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        let browser = surface_with_metadata(
            taskers_domain::PaneKind::Browser,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: true,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Waiting),
                latest_agent_message: Some("Should not surface".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(super::surface_activity_label(&terminal, now), None);
        assert_eq!(super::surface_status_label(&terminal, now), None);
        assert_eq!(super::surface_activity_label(&browser, now), None);
        assert_eq!(super::surface_status_label(&browser, now), None);
    }

    #[test]
    fn notification_rings_only_cover_agent_terminal_attention_states() {
        let waiting = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        let completed = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        let busy = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        let non_agent = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata::default(),
        );
        let browser = surface_with_metadata(
            taskers_domain::PaneKind::Browser,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        let mut waiting = waiting;
        waiting.attention = taskers_domain::AttentionState::WaitingInput;
        let mut completed = completed;
        completed.attention = taskers_domain::AttentionState::Completed;
        let mut busy = busy;
        busy.attention = taskers_domain::AttentionState::Busy;

        assert_eq!(
            super::surface_notification_ring(&waiting),
            Some(super::AttentionRingState::Waiting)
        );
        assert_eq!(
            super::surface_notification_ring(&completed),
            Some(super::AttentionRingState::Completed)
        );
        assert_eq!(super::surface_notification_ring(&busy), None);
        assert_eq!(super::surface_notification_ring(&non_agent), None);
        assert_eq!(super::surface_notification_ring(&browser), None);
    }

    #[test]
    fn status_label_survives_when_agent_has_no_latest_message() {
        let now = OffsetDateTime::now_utc();
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_kind: Some("codex".into()),
                agent_active: true,
                agent_state: Some(taskers_domain::WorkspaceAgentState::Waiting),
                latest_agent_message: Some("   ".into()),
                last_signal_at: Some(now),
                ..taskers_domain::PaneMetadata::default()
            },
        );

        assert_eq!(super::surface_activity_label(&surface, now), None);
        assert_eq!(
            super::surface_status_label(&surface, now).as_deref(),
            Some("Awaiting response")
        );
    }

    #[test]
    fn workspace_runtime_identity_prioritizes_failed_agents_over_idle_surfaces() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_pane_id = model.active_workspace().expect("workspace").active_pane;
        model
            .split_pane(
                workspace_id,
                Some(first_pane_id),
                taskers_domain::SplitAxis::Horizontal,
            )
            .expect("split pane");

        let now = OffsetDateTime::now_utc();
        let second_pane_id = {
            let workspace = model.workspaces.get(&workspace_id).expect("workspace");
            workspace
                .windows
                .get(&workspace.active_window)
                .expect("window")
                .layout
                .leaves()
                .into_iter()
                .find(|pane_id| *pane_id != first_pane_id)
                .expect("second pane")
        };

        {
            let workspace = model.workspaces.get_mut(&workspace_id).expect("workspace");
            let first_surface_id = workspace
                .panes
                .get(&first_pane_id)
                .map(|pane| pane.active_surface)
                .expect("first surface");
            let second_surface_id = workspace
                .panes
                .get(&second_pane_id)
                .map(|pane| pane.active_surface)
                .expect("second surface");
            let first_surface = workspace
                .panes
                .get_mut(&first_pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&first_surface_id))
                .expect("first surface record");
            first_surface.metadata.agent_kind = Some("codex".into());
            first_surface.metadata.agent_active = true;
            first_surface.metadata.agent_state = Some(taskers_domain::WorkspaceAgentState::Working);
            first_surface.metadata.last_signal_at = Some(now);
            first_surface.attention = taskers_domain::AttentionState::Busy;

            let second_surface = workspace
                .panes
                .get_mut(&second_pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&second_surface_id))
                .expect("second surface record");
            second_surface.metadata.agent_kind = Some("claude".into());
            second_surface.metadata.agent_active = false;
            second_surface.metadata.agent_state = Some(taskers_domain::WorkspaceAgentState::Failed);
            second_surface.metadata.last_signal_at = Some(now);
            second_surface.attention = taskers_domain::AttentionState::Error;
        }

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-runtime-priority",
        ));
        let snapshot = core.snapshot();
        let workspace_summary = snapshot.workspaces.first().expect("workspace summary");
        assert_eq!(workspace_summary.runtime.key, "claude");
        assert_eq!(
            workspace_summary.runtime.state,
            super::RuntimeStateSnapshot::Failed
        );

        let active_window = snapshot
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == snapshot.current_workspace.active_window_id)
            .expect("active window");
        assert_eq!(active_window.runtime.key, "claude");

        let first_pane =
            find_pane(&snapshot.current_workspace.layout, first_pane_id).expect("first pane");
        let second_pane =
            find_pane(&snapshot.current_workspace.layout, second_pane_id).expect("second pane");
        assert_eq!(first_pane.runtime.key, "codex");
        assert_eq!(
            first_pane.runtime.state,
            super::RuntimeStateSnapshot::Working
        );
        assert_eq!(second_pane.runtime.key, "claude");
        assert_eq!(
            second_pane.runtime.state,
            super::RuntimeStateSnapshot::Failed
        );
    }

    #[test]
    fn pane_notification_ring_prioritizes_inactive_agent_tabs() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let inactive_surface_id = model
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("inactive surface");
        let active_surface_id = model
            .create_surface(workspace_id, pane_id, taskers_domain::PaneKind::Terminal)
            .expect("create active terminal");

        {
            let workspace = model.workspaces.get_mut(&workspace_id).expect("workspace");
            let active_surface = workspace
                .panes
                .get_mut(&pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&active_surface_id))
                .expect("active surface record");
            active_surface.metadata.agent_kind = Some("codex".into());
            active_surface.attention = taskers_domain::AttentionState::Completed;

            let inactive_surface = workspace
                .panes
                .get_mut(&pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&inactive_surface_id))
                .expect("inactive surface record");
            inactive_surface.metadata.agent_kind = Some("claude".into());
            inactive_surface.attention = taskers_domain::AttentionState::Error;
        }

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-pane-notification-ring",
        ));
        let snapshot = core.snapshot();
        let pane = find_pane(&snapshot.current_workspace.layout, pane_id).expect("pane");
        let active_surface = pane
            .surfaces
            .iter()
            .find(|surface| surface.id == active_surface_id)
            .expect("active surface snapshot");
        let inactive_surface = pane
            .surfaces
            .iter()
            .find(|surface| surface.id == inactive_surface_id)
            .expect("inactive surface snapshot");

        assert_eq!(
            active_surface.notification_ring,
            Some(super::AttentionRingState::Completed)
        );
        assert_eq!(
            inactive_surface.notification_ring,
            Some(super::AttentionRingState::Error)
        );
        assert_eq!(
            pane.notification_ring,
            Some(super::AttentionRingState::Error)
        );
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
    fn portal_surface_frames_start_below_window_toolbar() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let metrics = LayoutMetrics::default();
        let min_content_y =
            snapshot.portal.content.y + metrics.window_toolbar_height + metrics.pane_header_height;

        assert!(
            snapshot
                .portal
                .panes
                .iter()
                .all(|plan| plan.frame.y >= min_content_y),
            "expected native surfaces to stay below window chrome"
        );
    }

    #[test]
    fn active_portal_surface_frame_matches_layout_insets() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let workspace = &snapshot.current_workspace;
        let metrics = LayoutMetrics::default();
        let active_window = workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == workspace.active_window_id)
            .expect("active window");
        let active_plan = snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == workspace.active_pane)
            .expect("active portal plan");
        let pane_frame = find_pane_frame(
            &active_window.layout,
            workspace.active_pane,
            workspace_window_content_frame(active_window.frame, metrics),
            metrics.split_gap,
        )
        .expect("active pane frame");
        let pane_kind = match &active_plan.mount {
            SurfaceMountSpec::Browser(_) => taskers_domain::PaneKind::Browser,
            SurfaceMountSpec::Terminal(_) => taskers_domain::PaneKind::Terminal,
        };
        let pane = find_pane(&workspace.layout, workspace.active_pane).expect("active pane");

        assert_eq!(
            active_plan.frame,
            pane_body_frame(
                pane_frame,
                metrics,
                &pane_kind,
                pane_shows_tab_strip_for_surface_count(pane.surfaces.len()),
            ),
            "expected native surface frame to match shell pane-body insets"
        );
    }

    #[test]
    fn multi_surface_pane_frames_include_tab_strip_height() {
        let core = SharedCore::bootstrap(bootstrap());
        let pane_id = core.snapshot().current_workspace.active_pane;

        core.dispatch_shell_action(ShellAction::AddTerminalSurface {
            pane_id: Some(pane_id),
        });

        let snapshot = core.snapshot();
        let workspace = &snapshot.current_workspace;
        let metrics = LayoutMetrics::default();
        let active_window = workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == workspace.active_window_id)
            .expect("active window");
        let active_plan = snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == pane_id)
            .expect("active portal plan");
        let pane_frame = find_pane_frame(
            &active_window.layout,
            pane_id,
            workspace_window_content_frame(active_window.frame, metrics),
            metrics.split_gap,
        )
        .expect("active pane frame");
        let pane_kind = match &active_plan.mount {
            SurfaceMountSpec::Browser(_) => taskers_domain::PaneKind::Browser,
            SurfaceMountSpec::Terminal(_) => taskers_domain::PaneKind::Terminal,
        };
        let pane = find_pane(&workspace.layout, pane_id).expect("active pane");

        assert_eq!(pane.surfaces.len(), 2);
        assert_eq!(
            active_plan.frame,
            pane_body_frame(
                pane_frame,
                metrics,
                &pane_kind,
                pane_shows_tab_strip_for_surface_count(pane.surfaces.len()),
            ),
            "expected multi-surface panes to reserve tab-strip height"
        );
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
                    if url == DEFAULT_BROWSER_HOME
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
            super::LayoutNodeSnapshot::Split { first, second, .. } => {
                [first.as_ref(), second.as_ref()]
                    .into_iter()
                    .find_map(|node| match node {
                        super::LayoutNodeSnapshot::Pane(pane) => pane
                            .surfaces
                            .iter()
                            .any(|surface| surface.id == browser_surface.surface_id)
                            .then_some(pane),
                        _ => None,
                    })
                    .expect("browser pane")
            }
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
    fn browser_snapshot_and_host_commands_follow_active_browser_surface() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });

        let snapshot = core.snapshot();
        let browser = snapshot.browser_chrome.expect("active browser chrome");
        assert_eq!(browser.url, DEFAULT_BROWSER_HOME);

        core.dispatch_shell_action(ShellAction::NavigateBrowser {
            surface_id: browser.surface_id,
            url: "about:blank".into(),
        });
        core.dispatch_shell_action(ShellAction::BrowserReload {
            surface_id: browser.surface_id,
        });
        core.dispatch_shell_action(ShellAction::BrowserBack {
            surface_id: browser.surface_id,
        });

        assert_eq!(
            core.drain_host_commands(),
            vec![
                HostCommand::BrowserNavigate {
                    surface_id: browser.surface_id,
                    url: "about:blank".into()
                },
                HostCommand::BrowserReload {
                    surface_id: browser.surface_id
                },
                HostCommand::BrowserBack {
                    surface_id: browser.surface_id
                },
            ]
        );
    }

    #[test]
    fn browser_catalog_keeps_background_browser_surfaces() {
        let core = SharedCore::bootstrap(bootstrap());
        let first_workspace_id = core.snapshot().current_workspace.id;
        let first_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: Some(first_pane_id),
        });

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let second_workspace_id = core.snapshot().current_workspace.id;
        let second_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: Some(second_pane_id),
        });

        let catalog = core.snapshot().browser_catalog;
        assert!(catalog.len() >= 2);
        assert!(
            catalog
                .iter()
                .any(|entry| entry.workspace_id == first_workspace_id)
        );
        assert!(
            catalog
                .iter()
                .any(|entry| entry.workspace_id == second_workspace_id)
        );
        assert!(catalog.iter().all(|entry| !entry.url.is_empty()));
    }

    #[test]
    fn terminal_catalog_keeps_background_terminal_surfaces() {
        let core = SharedCore::bootstrap(bootstrap());
        let first_workspace_id = core.snapshot().current_workspace.id;

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let second_workspace_id = core.snapshot().current_workspace.id;

        let catalog = core.snapshot().terminal_catalog;
        assert!(catalog.len() >= 2);
        assert!(
            catalog
                .iter()
                .any(|entry| entry.workspace_id == first_workspace_id)
        );
        assert!(
            catalog
                .iter()
                .any(|entry| entry.workspace_id == second_workspace_id)
        );
        assert!(catalog.iter().all(|entry| entry.spec.cols > 0));
        assert!(catalog.iter().all(|entry| entry.spec.rows > 0));
    }

    #[test]
    fn browser_navigation_host_events_update_browser_chrome_snapshot() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });

        let snapshot = core.snapshot();
        let browser = snapshot.browser_chrome.expect("active browser chrome");
        assert!(!browser.can_go_back);
        assert!(!browser.can_go_forward);
        assert!(!browser.devtools_open);

        core.apply_host_event(HostEvent::BrowserNavigationStateChanged {
            surface_id: browser.surface_id,
            can_go_back: true,
            can_go_forward: true,
            devtools_open: true,
        });

        let browser = core
            .snapshot()
            .browser_chrome
            .expect("active browser chrome after host event");
        assert!(browser.can_go_back);
        assert!(browser.can_go_forward);
        assert!(browser.devtools_open);
    }

    #[test]
    fn explicit_about_blank_browser_urls_are_preserved() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None });

        let browser = core
            .snapshot()
            .browser_chrome
            .expect("active browser chrome");
        core.apply_host_event(HostEvent::SurfaceUrlChanged {
            surface_id: browser.surface_id,
            url: "about:blank".into(),
        });

        let browser = core
            .snapshot()
            .browser_chrome
            .expect("active browser chrome");
        assert_eq!(browser.url, "about:blank");
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

    #[test]
    fn shell_drag_actions_update_snapshot_drag_mode() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let pane_id = snapshot.current_workspace.active_pane;
        let surface_id = snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == pane_id)
            .map(|plan| plan.surface_id)
            .expect("active surface");

        core.dispatch_shell_action(ShellAction::BeginSurfaceDrag {
            workspace_id,
            pane_id,
            surface_id,
        });
        assert_eq!(core.snapshot().drag_mode, ShellDragMode::Surface);
        assert_eq!(
            core.snapshot().surface_drag,
            Some(SurfaceDragSessionSnapshot {
                workspace_id,
                pane_id,
                surface_id,
                preview_workspace_id: workspace_id,
            })
        );

        core.dispatch_shell_action(ShellAction::BeginWindowDrag);
        assert_eq!(core.snapshot().drag_mode, ShellDragMode::Window);
        assert_eq!(core.snapshot().surface_drag, None);

        core.dispatch_shell_action(ShellAction::EndDrag);
        assert_eq!(core.snapshot().drag_mode, ShellDragMode::None);
        assert_eq!(core.snapshot().surface_drag, None);
    }

    #[test]
    fn surface_drag_preview_switches_workspace_and_cancel_restores_source() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_snapshot = core.snapshot();
        let source_workspace_id = source_snapshot.current_workspace.id;
        let source_pane_id = source_snapshot.current_workspace.active_pane;
        let source_surface_id = source_snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == source_pane_id)
            .map(|plan| plan.surface_id)
            .expect("active surface");

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core
            .snapshot()
            .workspaces
            .iter()
            .find(|workspace| workspace.id != source_workspace_id)
            .map(|workspace| workspace.id)
            .expect("target workspace");

        core.dispatch_shell_action(ShellAction::BeginSurfaceDrag {
            workspace_id: source_workspace_id,
            pane_id: source_pane_id,
            surface_id: source_surface_id,
        });
        core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace {
            workspace_id: target_workspace_id,
        });

        let preview_snapshot = core.snapshot();
        assert_eq!(preview_snapshot.current_workspace.id, target_workspace_id);
        assert_eq!(
            preview_snapshot.surface_drag,
            Some(SurfaceDragSessionSnapshot {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                surface_id: source_surface_id,
                preview_workspace_id: target_workspace_id,
            })
        );

        core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);

        let canceled_snapshot = core.snapshot();
        assert_eq!(canceled_snapshot.current_workspace.id, source_workspace_id);
        assert_eq!(canceled_snapshot.drag_mode, ShellDragMode::None);
        assert_eq!(canceled_snapshot.surface_drag, None);
    }

    #[test]
    fn end_drag_clears_surface_drag_without_restoring_source_workspace() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_snapshot = core.snapshot();
        let source_workspace_id = source_snapshot.current_workspace.id;
        let source_pane_id = source_snapshot.current_workspace.active_pane;
        let source_surface_id = source_snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == source_pane_id)
            .map(|plan| plan.surface_id)
            .expect("active surface");

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core
            .snapshot()
            .workspaces
            .iter()
            .find(|workspace| workspace.id != source_workspace_id)
            .map(|workspace| workspace.id)
            .expect("target workspace");

        core.dispatch_shell_action(ShellAction::BeginSurfaceDrag {
            workspace_id: source_workspace_id,
            pane_id: source_pane_id,
            surface_id: source_surface_id,
        });
        core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace {
            workspace_id: target_workspace_id,
        });
        core.dispatch_shell_action(ShellAction::EndDrag);

        let ended_snapshot = core.snapshot();
        assert_eq!(ended_snapshot.current_workspace.id, target_workspace_id);
        assert_eq!(ended_snapshot.drag_mode, ShellDragMode::None);
        assert_eq!(ended_snapshot.surface_drag, None);
    }

    #[test]
    fn external_app_state_mutations_advance_shared_core_revision() {
        let app_state = default_preview_app_state();
        let core = SharedCore::bootstrap(BootstrapModel {
            app_state: app_state.clone(),
            ..bootstrap()
        });
        let before = core.revision();

        let _ = app_state
            .dispatch(ControlCommand::CreateWorkspace {
                label: "External".into(),
            })
            .expect("external mutation");

        assert_eq!(core.revision(), before);
        core.sync_external_changes();
        assert!(core.revision() > before);
        assert!(
            core.snapshot()
                .workspaces
                .iter()
                .any(|workspace| workspace.title == "External")
        );
    }

    #[test]
    fn overview_mode_hides_live_portal_surfaces() {
        let core = SharedCore::bootstrap(bootstrap());
        assert!(!core.snapshot().portal.panes.is_empty());

        core.dispatch_shell_action(ShellAction::ToggleOverview);

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert!(snapshot.portal.panes.is_empty());
    }

    #[test]
    fn single_window_fills_normal_mode_viewport_when_attention_panel_hidden() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let workspace = &snapshot.current_workspace;
        let active_window = workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == workspace.active_window_id)
            .expect("active window");

        assert!(!snapshot.attention_panel_visible);
        assert_eq!(snapshot.portal.content.x, snapshot.metrics.sidebar_width);
        assert_eq!(snapshot.portal.content.y, snapshot.metrics.toolbar_height);
        assert_eq!(active_window.frame.x, snapshot.portal.content.x);
        assert_eq!(active_window.frame.y, snapshot.portal.content.y);
        assert_eq!(active_window.frame.width, snapshot.portal.content.width);
        assert_eq!(active_window.frame.height, snapshot.portal.content.height);
    }

    #[test]
    fn normal_mode_canvas_offsets_are_zero() {
        let snapshot = SharedCore::bootstrap(bootstrap()).snapshot();

        assert_eq!(snapshot.current_workspace.canvas_offset_x, 0);
        assert_eq!(snapshot.current_workspace.canvas_offset_y, 0);
    }

    #[test]
    fn overview_mode_uses_outer_padding() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        let snapshot = core.snapshot();
        let workspace = &snapshot.current_workspace;
        let active_window = workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == workspace.active_window_id)
            .expect("active window");

        assert!(snapshot.overview_mode);
        assert!(workspace.canvas_offset_x > 0);
        assert!(workspace.canvas_offset_y > 0);
        assert!(active_window.frame.x > workspace.viewport_origin_x);
        assert!(active_window.frame.y > workspace.viewport_origin_y);
    }

    #[test]
    fn attention_panel_visibility_tracks_activity_content() {
        let empty_snapshot = SharedCore::bootstrap(bootstrap()).snapshot();
        let unread_snapshot = SharedCore::bootstrap(bootstrap_with_notification(false)).snapshot();
        let done_snapshot = SharedCore::bootstrap(bootstrap_with_notification(true)).snapshot();

        assert!(!empty_snapshot.attention_panel_visible);
        assert!(empty_snapshot.activity.is_empty());
        assert!(empty_snapshot.done_activity.is_empty());

        assert!(unread_snapshot.attention_panel_visible);
        assert!(!unread_snapshot.activity.is_empty());
        assert!(unread_snapshot.portal.content.width < empty_snapshot.portal.content.width);

        assert!(!done_snapshot.attention_panel_visible);
        assert!(done_snapshot.activity.is_empty());
        assert!(!done_snapshot.done_activity.is_empty());
        assert_eq!(
            done_snapshot.portal.content.width,
            empty_snapshot.portal.content.width
        );
    }

    #[test]
    fn horizontal_scroll_host_events_pan_workspace_outside_overview() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let before = core.snapshot().current_workspace.viewport_x;

        core.apply_host_event(HostEvent::ViewportScrolled { dx: 180, dy: 0 });
        let after = core.snapshot().current_workspace.viewport_x;
        assert!(after > before);

        core.dispatch_shell_action(ShellAction::ToggleOverview);
        core.apply_host_event(HostEvent::ViewportScrolled { dx: 180, dy: 0 });

        let overview_snapshot = core.snapshot();
        assert_eq!(overview_snapshot.current_workspace.viewport_x, after);
    }

    #[test]
    fn browser_address_bar_normalizes_search_queries() {
        assert_eq!(resolved_browser_uri(""), "about:blank");
        assert_eq!(resolved_browser_uri("about:blank"), "about:blank");
        assert_eq!(
            resolved_browser_uri("file:///tmp/index.html"),
            "file:///tmp/index.html"
        );
        assert_eq!(
            resolved_browser_uri("rust"),
            "https://duckduckgo.com/?q=rust"
        );
        assert_eq!(
            resolved_browser_uri("cmux tabs"),
            "https://duckduckgo.com/?q=cmux+tabs"
        );
        assert_eq!(resolved_browser_uri("example.com"), "https://example.com");
        assert_eq!(
            resolved_browser_uri("localhost:3000"),
            "http://localhost:3000"
        );
    }

    #[test]
    fn scroll_viewport_is_clamped_to_canvas_bounds() {
        let core = SharedCore::bootstrap(bootstrap());

        core.dispatch_shell_action(ShellAction::ScrollViewport {
            dx: 50_000,
            dy: 50_000,
        });

        let snapshot = core.snapshot();
        assert!(snapshot.current_workspace.viewport_x >= 0);
        assert!(snapshot.current_workspace.viewport_x <= snapshot.current_workspace.canvas_width);
        assert!(snapshot.current_workspace.viewport_y >= 0);
        assert!(snapshot.current_workspace.viewport_y <= snapshot.current_workspace.canvas_height);
    }

    #[test]
    fn move_surface_shell_action_transfers_surface_between_panes() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });
        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::SplitTerminal {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let mut pane_ids = Vec::new();
        collect_pane_ids(&snapshot.current_workspace.layout, &mut pane_ids);
        let target_pane_id = pane_ids
            .into_iter()
            .find(|pane_id| *pane_id != source_pane_id)
            .expect("target pane");

        core.dispatch_shell_action(ShellAction::MoveSurface {
            surface_id: moved_surface_id,
            target_pane_id,
            target_index: 0,
        });

        let snapshot = core.snapshot();
        let source_pane =
            find_pane(&snapshot.current_workspace.layout, source_pane_id).expect("source pane");
        let target_pane =
            find_pane(&snapshot.current_workspace.layout, target_pane_id).expect("target pane");

        assert!(
            !source_pane
                .surfaces
                .iter()
                .any(|surface| surface.id == moved_surface_id)
        );
        assert_eq!(
            target_pane.surfaces.first().map(|surface| surface.id),
            Some(moved_surface_id)
        );
        assert_eq!(snapshot.current_workspace.active_pane, target_pane_id);
    }

    #[test]
    fn move_surface_shell_action_transfers_surface_between_workspaces() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_workspace_id = core.snapshot().current_workspace.id;
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core.snapshot().current_workspace.id;
        let target_pane_id = core.snapshot().current_workspace.active_pane;

        core.dispatch_shell_action(ShellAction::FocusWorkspace {
            workspace_id: source_workspace_id,
        });
        core.dispatch_shell_action(ShellAction::MoveSurface {
            surface_id: moved_surface_id,
            target_pane_id,
            target_index: usize::MAX,
        });

        let snapshot = core.snapshot();
        assert_eq!(snapshot.current_workspace.id, target_workspace_id);
        let target_pane =
            find_pane(&snapshot.current_workspace.layout, target_pane_id).expect("target pane");

        assert!(
            target_pane
                .surfaces
                .iter()
                .any(|surface| surface.id == moved_surface_id)
        );
        assert_eq!(target_pane.active_surface, moved_surface_id);
        assert_eq!(snapshot.current_workspace.active_pane, target_pane_id);
    }

    #[test]
    fn move_surface_to_split_shell_action_creates_neighbor_pane() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::MoveSurfaceToSplit {
            source_pane_id,
            surface_id: moved_surface_id,
            target_pane_id: source_pane_id,
            direction: Direction::Right,
        });

        let snapshot = core.snapshot();
        let mut pane_ids = Vec::new();
        collect_pane_ids(&snapshot.current_workspace.layout, &mut pane_ids);
        let new_pane_id = pane_ids
            .into_iter()
            .find(|pane_id| {
                *pane_id != source_pane_id
                    && find_pane(&snapshot.current_workspace.layout, *pane_id)
                        .is_some_and(|pane| pane.active_surface == moved_surface_id)
            })
            .expect("new pane");
        let source_pane =
            find_pane(&snapshot.current_workspace.layout, source_pane_id).expect("source pane");
        let target_pane =
            find_pane(&snapshot.current_workspace.layout, new_pane_id).expect("target pane");

        assert!(
            !source_pane
                .surfaces
                .iter()
                .any(|surface| surface.id == moved_surface_id)
        );
        assert_eq!(
            target_pane.surfaces.first().map(|surface| surface.id),
            Some(moved_surface_id)
        );
        assert_eq!(snapshot.current_workspace.active_pane, new_pane_id);
    }

    #[test]
    fn move_surface_to_split_shell_action_keeps_source_pane_live() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::MoveSurfaceToSplit {
            source_pane_id,
            surface_id: moved_surface_id,
            target_pane_id: source_pane_id,
            direction: Direction::Right,
        });

        let snapshot = core.snapshot();
        let source_pane =
            find_pane(&snapshot.current_workspace.layout, source_pane_id).expect("source pane");
        let remaining_surface_id = source_pane
            .surfaces
            .first()
            .map(|surface| surface.id)
            .expect("remaining surface");

        assert_eq!(source_pane.active_surface, remaining_surface_id);
        assert!(snapshot.portal.panes.iter().any(|plan| {
            plan.pane_id == source_pane_id && plan.surface_id == remaining_surface_id
        }));
        assert!(
            snapshot.portal.panes.iter().any(|plan| {
                plan.surface_id == moved_surface_id && plan.pane_id != source_pane_id
            })
        );
    }

    #[test]
    fn move_surface_to_split_shell_action_moves_surface_into_other_workspace() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_workspace_id = core.snapshot().current_workspace.id;
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core.snapshot().current_workspace.id;
        let target_pane_id = core.snapshot().current_workspace.active_pane;

        core.dispatch_shell_action(ShellAction::FocusWorkspace {
            workspace_id: source_workspace_id,
        });
        core.dispatch_shell_action(ShellAction::MoveSurfaceToSplit {
            source_pane_id,
            surface_id: moved_surface_id,
            target_pane_id,
            direction: Direction::Left,
        });

        let snapshot = core.snapshot();
        assert_eq!(snapshot.current_workspace.id, target_workspace_id);

        let mut pane_ids = Vec::new();
        collect_pane_ids(&snapshot.current_workspace.layout, &mut pane_ids);
        let new_pane_id = pane_ids
            .into_iter()
            .find(|pane_id| {
                *pane_id != target_pane_id
                    && find_pane(&snapshot.current_workspace.layout, *pane_id)
                        .is_some_and(|pane| pane.active_surface == moved_surface_id)
            })
            .expect("new pane");
        let new_pane =
            find_pane(&snapshot.current_workspace.layout, new_pane_id).expect("new pane");

        assert_eq!(
            new_pane.surfaces.first().map(|surface| surface.id),
            Some(moved_surface_id)
        );
        assert_eq!(snapshot.current_workspace.active_pane, new_pane_id);
    }

    #[test]
    fn move_surface_to_workspace_shell_action_switches_workspace_and_keeps_surface() {
        let core = SharedCore::bootstrap(bootstrap());
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
        });

        let snapshot = core.snapshot();
        let moved_surface_id = find_pane(&snapshot.current_workspace.layout, source_pane_id)
            .map(|pane| pane.active_surface)
            .expect("added surface");

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core
            .snapshot()
            .workspaces
            .iter()
            .find(|workspace| workspace.active)
            .map(|workspace| workspace.id)
            .expect("target workspace");

        core.dispatch_shell_action(ShellAction::MoveSurfaceToWorkspace {
            source_pane_id,
            surface_id: moved_surface_id,
            target_workspace_id,
        });

        let snapshot = core.snapshot();
        assert_eq!(snapshot.current_workspace.id, target_workspace_id);

        let mut pane_ids = Vec::new();
        collect_pane_ids(&snapshot.current_workspace.layout, &mut pane_ids);
        let moved_pane_id = pane_ids
            .into_iter()
            .find(|pane_id| {
                find_pane(&snapshot.current_workspace.layout, *pane_id)
                    .is_some_and(|pane| pane.active_surface == moved_surface_id)
            })
            .expect("moved pane");
        let moved_pane =
            find_pane(&snapshot.current_workspace.layout, moved_pane_id).expect("moved pane");

        assert_eq!(
            moved_pane.surfaces.first().map(|surface| surface.id),
            Some(moved_surface_id)
        );
        assert_eq!(snapshot.current_workspace.active_pane, moved_pane_id);
    }

    #[test]
    fn snapshot_exposes_workspace_agent_status_progress_and_log() {
        let app_state = default_preview_app_state();
        let workspace_id = app_state
            .snapshot_model()
            .active_workspace_id()
            .expect("workspace");
        let _ = app_state
            .dispatch(ControlCommand::AgentSetStatus {
                workspace_id,
                text: "Running agent sync".into(),
            })
            .expect("set status");
        let _ = app_state
            .dispatch(ControlCommand::AgentSetProgress {
                workspace_id,
                progress: taskers_domain::ProgressState {
                    value: 650,
                    label: Some("65%".into()),
                },
            })
            .expect("set progress");
        let _ = app_state
            .dispatch(ControlCommand::AgentAppendLog {
                workspace_id,
                entry: taskers_domain::WorkspaceLogEntry {
                    source: Some("codex".into()),
                    message: "Applied patch".into(),
                    created_at: OffsetDateTime::now_utc(),
                },
            })
            .expect("append log");

        let core = SharedCore::bootstrap(BootstrapModel {
            app_state,
            ..bootstrap()
        });
        let snapshot = core.snapshot();

        assert_eq!(
            snapshot.current_workspace_status.as_deref(),
            Some("Running agent sync")
        );
        assert_eq!(
            snapshot
                .current_workspace_progress
                .as_ref()
                .map(|progress| progress.label.as_deref()),
            Some(Some("65%"))
        );
        assert_eq!(snapshot.current_workspace_log.len(), 1);
        assert_eq!(
            snapshot.current_workspace_log[0].source.as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn agent_focus_latest_unread_command_switches_to_newest_workspace() {
        let app_state = default_preview_app_state();
        let core = SharedCore::bootstrap(BootstrapModel {
            app_state: app_state.clone(),
            ..bootstrap()
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let second_workspace_id = core.snapshot().current_workspace.id;
        let second_pane_id = core.snapshot().current_workspace.active_pane;
        let second_surface_id =
            find_pane(&core.snapshot().current_workspace.layout, second_pane_id)
                .map(|pane| pane.active_surface)
                .expect("second surface");
        let first_workspace_id = core
            .snapshot()
            .workspaces
            .iter()
            .find(|workspace| workspace.id != second_workspace_id)
            .map(|workspace| workspace.id)
            .expect("first workspace");

        core.dispatch_shell_action(ShellAction::FocusWorkspace {
            workspace_id: first_workspace_id,
        });
        let first_pane_id = core.snapshot().current_workspace.active_pane;
        let first_surface_id = find_pane(&core.snapshot().current_workspace.layout, first_pane_id)
            .map(|pane| pane.active_surface)
            .expect("first surface");

        let _ = app_state
            .dispatch(ControlCommand::AgentCreateNotification {
                target: taskers_domain::AgentTarget::Surface {
                    workspace_id: first_workspace_id,
                    pane_id: first_pane_id,
                    surface_id: first_surface_id,
                },
                kind: SignalKind::Notification,
                title: Some("Older".into()),
                subtitle: None,
                external_id: None,
                message: "Older".into(),
                state: DomainAttentionState::WaitingInput,
            })
            .expect("create older notification");
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = app_state
            .dispatch(ControlCommand::AgentCreateNotification {
                target: taskers_domain::AgentTarget::Surface {
                    workspace_id: second_workspace_id,
                    pane_id: second_pane_id,
                    surface_id: second_surface_id,
                },
                kind: SignalKind::Notification,
                title: Some("Newest".into()),
                subtitle: None,
                external_id: None,
                message: "Newest".into(),
                state: DomainAttentionState::WaitingInput,
            })
            .expect("create newest notification");
        core.sync_external_changes();

        core.dispatch_shortcut_action(super::ShortcutAction::FocusLatestUnread);

        assert_eq!(core.snapshot().current_workspace.id, second_workspace_id);
    }

    #[test]
    fn dismiss_activity_moves_notification_into_done_history() {
        let core = SharedCore::bootstrap(bootstrap_with_notification(false));
        let activity_id = core
            .snapshot()
            .activity
            .first()
            .map(|item| item.id)
            .expect("notification activity");

        core.dispatch_shell_action(ShellAction::DismissActivity { activity_id });

        let snapshot = core.snapshot();
        assert!(snapshot.activity.is_empty());
        assert_eq!(snapshot.done_activity.len(), 1);
        assert_eq!(snapshot.done_activity[0].id, activity_id);
        assert!(!snapshot.attention_panel_visible);
    }

    #[test]
    fn surface_flash_command_updates_pane_flash_token() {
        let app_state = default_preview_app_state();
        let snapshot_model = app_state.snapshot_model();
        let workspace_id = snapshot_model.active_workspace_id().expect("workspace");
        let pane_id = snapshot_model
            .active_workspace()
            .expect("workspace")
            .active_pane;
        let surface_id = snapshot_model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface");
        let _ = app_state
            .dispatch(ControlCommand::AgentTriggerFlash {
                workspace_id,
                pane_id,
                surface_id,
            })
            .expect("trigger flash");
        let core = SharedCore::bootstrap(BootstrapModel {
            app_state,
            ..bootstrap()
        });
        let snapshot = core.snapshot();
        let pane = find_pane(&snapshot.current_workspace.layout, pane_id).expect("pane");
        assert!(pane.focus_flash_token > 0);
    }
}
