use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    path::PathBuf,
    sync::Arc,
};
use taskers_control::{ControlCommand, ControlResponse, VcsCommandResult};
use taskers_core::{AppState, default_session_path};
use taskers_domain::{
    ActivityItem, AppModel, DEFAULT_WORKSPACE_WINDOW_GAP, KEYBOARD_RESIZE_STEP,
    MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_WIDTH, NotificationId, PaneMetadata,
    PaneMetadataPatch, SplitAxis as DomainSplitAxis, SurfaceRecord, WindowFrame, Workspace,
    WorkspaceSummary as DomainWorkspaceSummary, WorkspaceWindowTabRecord,
};
use taskers_ghostty::{BackendChoice, SurfaceDescriptor};
use taskers_runtime::{ShellLaunchSpec, default_shell_program};
use time::OffsetDateTime;
use tokio::sync::watch;

pub use taskers_control::{
    VcsCommand, VcsCommitEntry, VcsFileEntry, VcsFileStatus, VcsMode, VcsSnapshot,
};
pub use taskers_domain::{
    BrowserProfileMode, Direction, PaneContainerId, PaneId, PaneKind, PaneTabId, PaneTabLayoutNode,
    SurfaceId, WorkspaceColumnId, WorkspaceId, WorkspaceWindowId, WorkspaceWindowMoveTarget,
    WorkspaceWindowTabId,
};

pub const MIN_RENDERED_NATIVE_SURFACE_WIDTH_PX: i32 = 160;
pub const MIN_RENDERED_NATIVE_SURFACE_HEIGHT_PX: i32 = 96;
pub const MIN_WORKSPACE_WINDOW_GAP: i32 = 0;
pub const MAX_WORKSPACE_WINDOW_GAP: i32 = 64;
const EXPANDED_ACTIVE_SPLIT_RATIO: u16 = 999;
const COLLAPSED_INACTIVE_SPLIT_RATIO: u16 = 1;

pub fn clamp_workspace_window_gap(gap: i32) -> i32 {
    gap.clamp(MIN_WORKSPACE_WINDOW_GAP, MAX_WORKSPACE_WINDOW_GAP)
}

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
                "Keep common focus, split resize, top-level window, overview, and close actions bound."
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
    FitTerminalToViewport,
    SplitRight,
    SplitDown,
}

impl ShortcutAction {
    pub const ALL: [Self; 30] = [
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
        Self::FitTerminalToViewport,
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
            Self::FitTerminalToViewport => "fit_terminal_to_viewport",
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
            Self::ResizeSplitLeft => "Make terminal narrower",
            Self::ResizeSplitRight => "Make terminal wider",
            Self::ResizeSplitUp => "Make split shorter",
            Self::ResizeSplitDown => "Make split taller",
            Self::FitTerminalToViewport => "Fit terminal to viewport",
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
            Self::NewWindowLeft => {
                "Create a top-level window on the left using half the active window width."
            }
            Self::NewWindowRight => {
                "Create a top-level window on the right using half the active window width."
            }
            Self::NewWindowUp => {
                "Create a stacked top-level window above using half the active window height."
            }
            Self::NewWindowDown => {
                "Create a stacked top-level window below using half the active window height."
            }
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
            Self::ResizeSplitLeft => {
                "Reduce the active split width, or the active window width when no split can resize."
            }
            Self::ResizeSplitRight => {
                "Increase the active split width, or the active window width when no split can resize."
            }
            Self::ResizeSplitUp => "Reduce the active split height.",
            Self::ResizeSplitDown => "Increase the active split height.",
            Self::FitTerminalToViewport => {
                "Resize the active terminal so it fills the visible workspace terminal area."
            }
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
            | Self::ResizeSplitDown
            | Self::FitTerminalToViewport => "Advanced resize",
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
                Self::ResizeSplitLeft => &["<Control><Alt>minus", "<Control><Alt>KP_Subtract"],
                Self::ResizeSplitRight => &["<Control><Alt>equal", "<Control><Alt>KP_Add"],
                Self::ResizeSplitUp => &[],
                Self::ResizeSplitDown => &[],
                Self::FitTerminalToViewport => &["<Control><Alt>0", "<Control><Alt>KP_0"],
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
                Self::ResizeSplitLeft => &[
                    "<Control><Alt>minus",
                    "<Control><Alt>KP_Subtract",
                    "<Control><Alt><Shift>Home",
                ],
                Self::ResizeSplitRight => &[
                    "<Control><Alt>equal",
                    "<Control><Alt>KP_Add",
                    "<Control><Alt><Shift>End",
                ],
                Self::ResizeSplitUp => &["<Control><Alt><Shift>Page_Up"],
                Self::ResizeSplitDown => &["<Control><Alt><Shift>Page_Down"],
                Self::FitTerminalToViewport => &["<Control><Alt>0", "<Control><Alt>KP_0"],
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
    pub terminal_persistence: RuntimeCapability,
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
            terminal_persistence: unavailable(),
        }
    }
}

#[derive(Clone)]
pub struct BootstrapModel {
    pub app_state: AppState,
    pub runtime_status: RuntimeStatus,
    pub selected_theme_id: String,
    pub selected_shortcut_preset: ShortcutPreset,
    pub configured_shell: Option<String>,
    pub embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot,
    pub notification_preferences: NotificationPreferencesSnapshot,
    pub render_live_surfaces_in_overview: bool,
    pub workspace_window_gap: i32,
}

impl Default for BootstrapModel {
    fn default() -> Self {
        Self {
            app_state: default_preview_app_state(),
            runtime_status: RuntimeStatus::default(),
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: ShortcutPreset::Balanced,
            configured_shell: None,
            embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot::default(),
            notification_preferences: NotificationPreferencesSnapshot::default(),
            render_live_surfaces_in_overview: true,
            workspace_window_gap: DEFAULT_WORKSPACE_WINDOW_GAP,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptionalSettingChoice {
    #[default]
    Default,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedTerminalTextSettingKey {
    Theme,
    FontFamily,
    FontSize,
    WindowPaddingX,
    WindowPaddingY,
    CursorStyle,
    ScrollbackLimit,
    BackgroundOpacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedTerminalBoolSettingKey {
    CursorStyleBlink,
    BackgroundOpacityCells,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmbeddedTerminalSettingsSnapshot {
    pub theme: String,
    pub font_family: String,
    pub font_size: String,
    pub window_padding_x: String,
    pub window_padding_y: String,
    pub cursor_style: String,
    pub cursor_style_blink: OptionalSettingChoice,
    pub scrollback_limit: String,
    pub background_opacity: String,
    pub background_opacity_cells: OptionalSettingChoice,
    pub base_config_path: String,
    pub override_config_path: String,
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

    pub const fn right(self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(self) -> i32 {
        self.y + self.height
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

    pub fn inset_horizontal(self, amount: i32) -> Self {
        let clamped = amount.clamp(0, self.width.saturating_sub(1) / 2);
        Self {
            x: self.x + clamped,
            y: self.y,
            width: (self.width - clamped * 2).max(1),
            height: self.height,
        }
    }
}

const RESIZE_HANDLE_THICKNESS_PX: i32 = 12;
const RESIZE_CORNER_SIZE_PX: i32 = 16;
const WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX: i32 = 8;

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
    pub terminal_gutter_x: i32,
}

impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            sidebar_width: 200,
            activity_width: 264,
            toolbar_height: 28,
            workspace_padding: 8,
            window_border_width: 1,
            window_toolbar_height: 20,
            window_body_padding: 0,
            split_gap: 2,
            pane_border_width: 1,
            pane_header_height: 24,
            browser_toolbar_height: 30,
            surface_tab_height: 24,
            terminal_gutter_x: 0,
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
    pub browser_profile_mode: BrowserProfileMode,
    pub cwd: Option<String>,
    pub attention: AttentionState,
    pub notification_ring: Option<AttentionRingState>,
    pub interrupted_agent_resume: Option<InterruptedAgentResumeSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterruptedAgentResumeSnapshot {
    pub title: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LivePaneSnapshot {
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
pub struct PaneTabSnapshot {
    pub id: PaneTabId,
    pub active: bool,
    pub attention: AttentionState,
    pub runtime: RuntimeIdentitySnapshot,
    pub title: String,
    pub pane_count: usize,
    pub surface_count: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaneSnapshot {
    pub id: PaneId,
    pub pane_container_id: PaneContainerId,
    pub active: bool,
    pub attention: AttentionState,
    pub notification_ring: Option<AttentionRingState>,
    pub active_pane_tab: PaneTabId,
    pub active_surface: SurfaceId,
    pub runtime: RuntimeIdentitySnapshot,
    pub surfaces: Vec<SurfaceSnapshot>,
    pub pane_tabs: Vec<PaneTabSnapshot>,
    pub layout: PaneTabLayoutSnapshot,
    pub focus_flash_token: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaneTabLayoutSnapshot {
    Pane(LivePaneSnapshot),
    Split {
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneTabLayoutSnapshot>,
        second: Box<PaneTabLayoutSnapshot>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserMountSpec {
    pub url: String,
    pub profile_mode: BrowserProfileMode,
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
    pub notification_ring: Option<AttentionRingState>,
    pub pane_frame: Frame,
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
    pub profile_mode: BrowserProfileMode,
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
    pub overview_scene: OverviewSceneSnapshot,
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
    pub active_tab: WorkspaceWindowTabId,
    pub active_pane: PaneId,
    pub frame: Frame,
    pub tabs: Vec<WorkspaceWindowTabSnapshot>,
    pub layout: LayoutNodeSnapshot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceWindowTabSnapshot {
    pub id: WorkspaceWindowTabId,
    pub active: bool,
    pub attention: AttentionState,
    pub runtime: RuntimeIdentitySnapshot,
    pub title: String,
    pub pane_count: usize,
    pub surface_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverviewPreviewModeSnapshot {
    Summary,
    LivePreferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewWindowCardSnapshot {
    pub window_id: WorkspaceWindowId,
    pub column_id: WorkspaceColumnId,
    pub title: String,
    pub runtime: RuntimeIdentitySnapshot,
    pub attention: AttentionState,
    pub active: bool,
    pub pane_count: usize,
    pub surface_count: usize,
    pub tab_count: usize,
    pub preview_mode: OverviewPreviewModeSnapshot,
    pub preview_lines: Vec<String>,
    pub can_move_left: bool,
    pub can_move_right: bool,
    pub can_move_up: bool,
    pub can_move_down: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverviewSceneSnapshot {
    pub prefer_live_preview: bool,
    pub cards: Vec<OverviewWindowCardSnapshot>,
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
    pub profile_mode: BrowserProfileMode,
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
    WindowTab,
    PaneTab,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowTabDragSessionSnapshot {
    pub window_id: WorkspaceWindowId,
    pub tab_id: WorkspaceWindowTabId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneTabDragSessionSnapshot {
    pub pane_container_id: PaneContainerId,
    pub pane_tab_id: PaneTabId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SurfaceDragSessionSnapshot {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub preview_workspace_id: WorkspaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragSessionSnapshot {
    WindowTab(WindowTabDragSessionSnapshot),
    PaneTab(PaneTabDragSessionSnapshot),
    Surface(SurfaceDragSessionSnapshot),
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
    pub configured_shell: Option<String>,
    pub default_shell_label: String,
    pub embedded_terminal: EmbeddedTerminalSettingsSnapshot,
    pub notification_preferences: NotificationPreferencesSnapshot,
    pub render_live_surfaces_in_overview: bool,
    pub workspace_window_gap: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsPanelSnapshot {
    pub visible: bool,
    pub target_surface_id: Option<SurfaceId>,
    pub target_surface_title: Option<String>,
    pub snapshot: Option<VcsSnapshot>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShellSnapshot {
    pub revision: u64,
    pub section: ShellSection,
    pub overview_mode: bool,
    pub drag_mode: ShellDragMode,
    pub drag_session: Option<DragSessionSnapshot>,
    pub surface_drag: Option<SurfaceDragSessionSnapshot>,
    pub resize_preview_active: bool,
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
    pub resize_handles: Vec<ResizeHandleSnapshot>,
    pub metrics: LayoutMetrics,
    pub runtime_status: RuntimeStatus,
    pub settings: SettingsSnapshot,
    pub vcs_panel: VcsPanelSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeHandleCursor {
    EastWest,
    NorthSouth,
    SouthEast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResizeHandleSnapshot {
    pub id: String,
    pub frame: Frame,
    pub cursor: ResizeHandleCursor,
    pub target: ResizeHandleTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceOuterEdge {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResizeHandleTarget {
    WorkspaceColumnEdge {
        workspace_id: WorkspaceId,
        column_widths: Vec<(WorkspaceColumnId, i32)>,
        leading_index: usize,
    },
    WorkspaceColumnOuterEdge {
        workspace_id: WorkspaceId,
        column_widths: Vec<(WorkspaceColumnId, i32)>,
        column_index: usize,
        edge: WorkspaceOuterEdge,
    },
    WorkspaceWindowBottomEdge {
        workspace_id: WorkspaceId,
        window_heights: Vec<(WorkspaceWindowId, i32)>,
        upper_index: usize,
    },
    WorkspaceWindowCorner {
        workspace_id: WorkspaceId,
        column_widths: Vec<(WorkspaceColumnId, i32)>,
        leading_index: usize,
        window_heights: Vec<(WorkspaceWindowId, i32)>,
        upper_index: usize,
    },
    WorkspaceWindowSplit {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        path: Vec<bool>,
        axis: SplitAxis,
        parent_frame: Frame,
        initial_ratio: u16,
    },
    PaneTabSplit {
        workspace_id: WorkspaceId,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        path: Vec<bool>,
        axis: SplitAxis,
        parent_frame: Frame,
        initial_ratio: u16,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResizePreview {
    WorkspaceColumnWidths {
        workspace_id: WorkspaceId,
        widths: Vec<(WorkspaceColumnId, i32)>,
    },
    WorkspaceWindowHeights {
        workspace_id: WorkspaceId,
        heights: Vec<(WorkspaceWindowId, i32)>,
    },
    WorkspaceWindowCorner {
        workspace_id: WorkspaceId,
        column_widths: Vec<(WorkspaceColumnId, i32)>,
        window_heights: Vec<(WorkspaceWindowId, i32)>,
    },
    WorkspaceWindowSplitRatio {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        path: Vec<bool>,
        ratio: u16,
    },
    PaneTabSplitRatio {
        workspace_id: WorkspaceId,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        path: Vec<bool>,
        ratio: u16,
    },
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
    BrowserClearData { surface_id: SurfaceId },
    TerminalSendText { surface_id: SurfaceId, text: String },
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
    CreateWorkspaceWindowTab {
        window_id: WorkspaceWindowId,
    },
    FocusWorkspaceWindowTab {
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    },
    MoveWorkspaceWindowTab {
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target_index: usize,
    },
    TransferWorkspaceWindowTab {
        source_window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target_window_id: WorkspaceWindowId,
        target_index: usize,
    },
    ExtractWorkspaceWindowTab {
        source_window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target: WorkspaceWindowMoveTarget,
    },
    CloseWorkspaceWindowTab {
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    },
    CreatePaneTab {
        pane_container_id: PaneContainerId,
        kind: PaneKind,
    },
    FocusPaneTab {
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    },
    MovePaneTab {
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        target_index: usize,
    },
    TransferPaneTab {
        source_pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        target_pane_container_id: PaneContainerId,
        target_index: usize,
    },
    ClosePaneTab {
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    },
    ScrollViewport {
        dx: i32,
        dy: i32,
    },
    SplitBrowser {
        pane_id: Option<PaneId>,
        profile_mode: BrowserProfileMode,
    },
    SplitTerminal {
        pane_id: Option<PaneId>,
    },
    AddBrowserSurface {
        pane_id: Option<PaneId>,
        profile_mode: BrowserProfileMode,
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
    BeginWindowTabDrag {
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    },
    BeginPaneTabDrag {
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    },
    BeginSurfaceDrag {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    PreviewSurfaceDragWorkspace {
        workspace_id: WorkspaceId,
    },
    PreviewResize {
        preview: ResizePreview,
    },
    CommitResizePreview,
    CancelResizePreview,
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
    ClearBrowserData {
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
    DismissSurfaceAlert {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    ResumeInterruptedAgent {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    DismissInterruptedAgentResume {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    ToggleVcsPanel,
    RefreshVcsPanel,
    ShowVcsDiff {
        path: Option<String>,
    },
    RunVcsCommand {
        command: VcsCommand,
    },
    SelectTheme {
        theme_id: String,
    },
    SelectShortcutPreset {
        preset_id: String,
    },
    SetConfiguredShell {
        shell: Option<String>,
    },
    SetEmbeddedTerminalTextSetting {
        key: EmbeddedTerminalTextSettingKey,
        value: String,
    },
    SetEmbeddedTerminalBoolSetting {
        key: EmbeddedTerminalBoolSettingKey,
        value: OptionalSettingChoice,
    },
    SetNotificationPreference {
        key: NotificationPreferenceKey,
        enabled: bool,
    },
    SetOverviewLiveSurfaces {
        enabled: bool,
    },
    SetWorkspaceWindowGap {
        gap: i32,
    },
}

#[derive(Debug, Clone)]
struct UiState {
    section: ShellSection,
    overview_mode: bool,
    drag_mode: ShellDragMode,
    drag_session: Option<DragSessionSnapshot>,
    resize_preview: Option<ResizePreview>,
    selected_theme_id: String,
    selected_shortcut_preset: ShortcutPreset,
    configured_shell: Option<String>,
    embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot,
    notification_preferences: NotificationPreferencesSnapshot,
    render_live_surfaces_in_overview: bool,
    workspace_window_gap: i32,
    window_size: PixelSize,
    vcs_panel_visible: bool,
    last_terminal_surface_by_workspace: BTreeMap<WorkspaceId, SurfaceId>,
    vcs_snapshot: Option<VcsSnapshot>,
    vcs_error: Option<String>,
    vcs_diff_path: Option<String>,
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
    outer_padding_x: i32,
    outer_padding_y: i32,
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
        let mut core = Self {
            app_state: bootstrap.app_state,
            revision,
            observed_app_revision,
            metrics: LayoutMetrics::default(),
            runtime_status: bootstrap.runtime_status,
            ui: UiState {
                section: ShellSection::Workspace,
                overview_mode: false,
                drag_mode: ShellDragMode::None,
                drag_session: None,
                resize_preview: None,
                selected_theme_id: bootstrap.selected_theme_id,
                selected_shortcut_preset: bootstrap.selected_shortcut_preset,
                configured_shell: normalize_configured_shell(bootstrap.configured_shell.as_deref()),
                embedded_terminal_settings: bootstrap.embedded_terminal_settings,
                notification_preferences: bootstrap.notification_preferences,
                render_live_surfaces_in_overview: bootstrap.render_live_surfaces_in_overview,
                workspace_window_gap: clamp_workspace_window_gap(bootstrap.workspace_window_gap),
                window_size: PixelSize::new(1440, 900),
                vcs_panel_visible: false,
                last_terminal_surface_by_workspace: BTreeMap::new(),
                vcs_snapshot: None,
                vcs_error: None,
                vcs_diff_path: None,
            },
            host_commands: VecDeque::new(),
            browser_navigation: BTreeMap::new(),
        };
        let _ = core.bootstrap_active_workspace_top_level_extents_if_needed();
        core
    }

    fn revision(&self) -> u64 {
        self.revision
    }

    fn model_for_snapshot(&self) -> AppModel {
        let mut model = self.app_state.snapshot_model();
        if let Some(preview) = &self.ui.resize_preview {
            apply_resize_preview_to_model(&mut model, preview);
        }
        model
    }

    fn snapshot(&self) -> ShellSnapshot {
        let model = self.model_for_snapshot();
        let agents = self.agent_sessions_snapshot(&model);
        let activity = self.activity_snapshot(&model);
        let done_activity = self.done_activity_snapshot(&model);
        let attention_panel_visible = !agents.is_empty() || !activity.is_empty();
        let right_panel_visible = attention_panel_visible || self.ui.vcs_panel_visible;
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
        let viewport = self.workspace_viewport_frame(right_panel_visible);
        let clamped_viewport = clamped_workspace_viewport(
            workspace,
            viewport.width,
            viewport.height,
            self.ui.workspace_window_gap,
            workspace.viewport.clone(),
        );
        let render_context = workspace_render_context(
            workspace,
            self.ui.overview_mode,
            viewport.width,
            viewport.height,
            self.metrics,
            self.ui.workspace_window_gap,
        );
        let placements = workspace_display_window_placements(
            workspace,
            render_context,
            self.ui.workspace_window_gap,
        );
        let canvas_metrics = workspace_canvas_metrics(
            &placements,
            render_context.outer_padding_x,
            render_context.outer_padding_y,
        );
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
        let columns = self.workspace_columns_snapshot(workspace, &window_frames);
        let overview_scene =
            overview_scene_snapshot(&columns, self.ui.render_live_surfaces_in_overview);
        let resize_handles = if matches!(self.ui.section, ShellSection::Workspace)
            && !self.ui.overview_mode
            && self.ui.drag_mode == ShellDragMode::None
        {
            self.resize_handles_snapshot(workspace_id, workspace, &window_frames)
        } else {
            Vec::new()
        };

        ShellSnapshot {
            revision: self.revision,
            section: self.ui.section,
            overview_mode: self.ui.overview_mode,
            drag_mode: self.ui.drag_mode,
            drag_session: self.ui.drag_session,
            surface_drag: match self.ui.drag_session {
                Some(DragSessionSnapshot::Surface(session)) => Some(session),
                _ => None,
            },
            resize_preview_active: self.ui.resize_preview.is_some(),
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
                overview_scene,
                columns,
                layout: self.snapshot_layout(
                    workspace,
                    active_window
                        .active_layout()
                        .expect("active workspace window tab should exist"),
                ),
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
                    && (!self.ui.overview_mode || self.ui.render_live_surfaces_in_overview)
                {
                    self.collect_workspace_surface_plans(workspace_id, workspace, &window_frames)
                } else {
                    Vec::new()
                },
            },
            resize_handles,
            metrics: self.metrics,
            runtime_status: self.runtime_status.clone(),
            settings: self.settings_snapshot(),
            vcs_panel: self.vcs_panel_snapshot(&model),
        }
    }

    fn workspace_viewport_frame(&self, right_panel_visible: bool) -> Frame {
        let metrics = self.metrics;
        let activity_width = if right_panel_visible {
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
            configured_shell: self.ui.configured_shell.clone(),
            default_shell_label: default_shell_program().display().to_string(),
            embedded_terminal: self.ui.embedded_terminal_settings.clone(),
            notification_preferences: self.ui.notification_preferences,
            render_live_surfaces_in_overview: self.ui.render_live_surfaces_in_overview,
            workspace_window_gap: self.ui.workspace_window_gap,
        }
    }

    fn preview_resize(&mut self, preview: ResizePreview) -> bool {
        if self.ui.resize_preview.as_ref() == Some(&preview) {
            return false;
        }
        self.ui.resize_preview = Some(preview);
        self.bump_local_revision();
        true
    }

    fn commit_resize_preview(&mut self) -> bool {
        let Some(preview) = self.ui.resize_preview.clone() else {
            return false;
        };

        let committed = match preview {
            ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths,
            } => widths.into_iter().all(|(workspace_column_id, width)| {
                self.dispatch_control(ControlCommand::SetWorkspaceColumnWidth {
                    workspace_id,
                    workspace_column_id,
                    width,
                })
            }),
            ResizePreview::WorkspaceWindowHeights {
                workspace_id,
                heights,
            } => heights.into_iter().all(|(workspace_window_id, height)| {
                self.dispatch_control(ControlCommand::SetWorkspaceWindowHeight {
                    workspace_id,
                    workspace_window_id,
                    height,
                })
            }),
            ResizePreview::WorkspaceWindowCorner {
                workspace_id,
                column_widths,
                window_heights,
            } => {
                column_widths
                    .into_iter()
                    .all(|(workspace_column_id, width)| {
                        self.dispatch_control(ControlCommand::SetWorkspaceColumnWidth {
                            workspace_id,
                            workspace_column_id,
                            width,
                        })
                    })
                    && window_heights
                        .into_iter()
                        .all(|(workspace_window_id, height)| {
                            self.dispatch_control(ControlCommand::SetWorkspaceWindowHeight {
                                workspace_id,
                                workspace_window_id,
                                height,
                            })
                        })
            }
            ResizePreview::WorkspaceWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path,
                ratio,
            } => self.dispatch_control(ControlCommand::SetWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path,
                ratio,
            }),
            ResizePreview::PaneTabSplitRatio {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path,
                ratio,
            } => self.dispatch_control(ControlCommand::SetPaneTabSplitRatio {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path,
                ratio,
            }),
        };

        if committed {
            self.ui.resize_preview = None;
        }

        committed
    }

    fn cancel_resize_preview(&mut self) -> bool {
        if self.ui.resize_preview.take().is_none() {
            return false;
        }
        self.bump_local_revision();
        true
    }

    fn vcs_panel_snapshot(&self, model: &AppModel) -> VcsPanelSnapshot {
        let workspace_id = model.active_workspace_id();
        let target_surface_id =
            workspace_id.and_then(|workspace_id| self.vcs_target_surface_id(model, workspace_id));
        let target_surface_title = target_surface_id.and_then(|surface_id| {
            model
                .workspaces
                .values()
                .flat_map(|workspace| workspace.panes.values())
                .flat_map(|pane| pane.surfaces.values())
                .find(|surface| surface.id == surface_id)
                .and_then(|surface| {
                    surface
                        .metadata
                        .title
                        .clone()
                        .or_else(|| surface.metadata.cwd.clone())
                })
        });
        VcsPanelSnapshot {
            visible: self.ui.vcs_panel_visible,
            target_surface_id,
            target_surface_title,
            snapshot: self.ui.vcs_snapshot.clone(),
            error: self.ui.vcs_error.clone(),
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
        let active_tab = window
            .active_tab_record()
            .expect("workspace window should have an active tab");
        let pane_container_ids = active_tab.layout.leaves();
        let pane_count = pane_container_ids
            .iter()
            .filter_map(|pane_container_id| workspace.pane_containers.get(pane_container_id))
            .flat_map(|pane_container| pane_container.tabs.values())
            .map(|pane_tab| pane_tab.layout.leaves().len())
            .sum();
        let surface_count = pane_container_ids
            .iter()
            .filter_map(|pane_container_id| workspace.pane_containers.get(pane_container_id))
            .flat_map(|pane_container| pane_container.tabs.values())
            .flat_map(|pane_tab| pane_tab.layout.leaves())
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .map(|pane| pane.surfaces.len())
            .sum();
        let title = window_primary_title(workspace, window);
        let tabs = window
            .tabs
            .values()
            .map(|tab| workspace_window_tab_snapshot(workspace, tab, window.active_tab, now))
            .collect();

        WorkspaceWindowSnapshot {
            id: window.id,
            column_id,
            active: workspace.active_window == window.id,
            attention: workspace_window_attention(workspace, window),
            runtime: workspace_window_runtime_identity(workspace, window, now),
            title,
            pane_count,
            surface_count,
            active_tab: window.active_tab,
            active_pane: window
                .active_pane()
                .expect("workspace window should have an active pane"),
            frame,
            tabs,
            layout: self.snapshot_layout(workspace, &active_tab.layout),
        }
    }

    fn snapshot_layout(
        &self,
        workspace: &Workspace,
        node: &taskers_domain::LayoutNode,
    ) -> LayoutNodeSnapshot {
        match node {
            taskers_domain::LayoutNode::Leaf { leaf_id } => LayoutNodeSnapshot::Pane(
                self.pane_snapshot(
                    workspace,
                    workspace
                        .pane_containers
                        .get(leaf_id)
                        .expect("layout leaf should reference a pane container"),
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
        pane_container: &taskers_domain::PaneContainerRecord,
    ) -> PaneSnapshot {
        let active_pane_tab = pane_container
            .active_tab_record()
            .expect("pane container should have an active pane tab");
        let active_pane = workspace
            .panes
            .get(&active_pane_tab.active_pane)
            .expect("active pane tab should reference a pane");
        let active_live_pane = self.live_pane_snapshot(workspace, active_pane);
        let pane_tabs = pane_container
            .tabs
            .values()
            .map(|pane_tab| self.pane_tab_snapshot(workspace, pane_tab, pane_container.active_tab))
            .collect::<Vec<_>>();
        let is_active = workspace.active_pane == active_pane.id;
        let has_unread = pane_container
            .tabs
            .values()
            .flat_map(|pane_tab| pane_tab.layout.leaves())
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .map(taskers_domain::PaneRecord::highest_attention)
            .max_by_key(|attention| attention.rank())
            .unwrap_or(taskers_domain::AttentionState::Normal)
            != taskers_domain::AttentionState::Normal;
        let explicit_flash_token = pane_container
            .tabs
            .values()
            .flat_map(|pane_tab| pane_tab.layout.leaves())
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .flat_map(|pane| pane.surfaces.values())
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
        PaneSnapshot {
            id: active_pane.id,
            pane_container_id: pane_container.id,
            active: is_active,
            attention: pane_container_attention(workspace, pane_container).into(),
            notification_ring: pane_container_notification_ring(workspace, pane_container),
            active_pane_tab: pane_container.active_tab,
            active_surface: active_live_pane.active_surface,
            runtime: active_live_pane.runtime.clone(),
            surfaces: active_live_pane.surfaces.clone(),
            pane_tabs,
            layout: self.pane_tab_layout_snapshot(workspace, &active_pane_tab.layout),
            focus_flash_token: flash_token,
        }
    }

    fn pane_tab_snapshot(
        &self,
        workspace: &Workspace,
        pane_tab: &taskers_domain::PaneTabRecord,
        active_pane_tab_id: PaneTabId,
    ) -> PaneTabSnapshot {
        let now = OffsetDateTime::now_utc();
        let pane_ids = pane_tab.layout.leaves();
        let pane_count = pane_ids.len();
        let surface_count = pane_ids
            .iter()
            .filter_map(|pane_id| workspace.panes.get(pane_id))
            .map(|pane| pane.surfaces.len())
            .sum();
        PaneTabSnapshot {
            id: pane_tab.id,
            active: pane_tab.id == active_pane_tab_id,
            attention: pane_tab_attention(workspace, pane_tab).into(),
            runtime: pane_tab_runtime_identity(workspace, pane_tab, now),
            title: pane_tab_primary_title(workspace, pane_tab),
            pane_count,
            surface_count,
        }
    }

    fn pane_tab_layout_snapshot(
        &self,
        workspace: &Workspace,
        node: &taskers_domain::PaneTabLayoutNode,
    ) -> PaneTabLayoutSnapshot {
        match node {
            taskers_domain::PaneTabLayoutNode::Leaf { leaf_id } => PaneTabLayoutSnapshot::Pane(
                self.live_pane_snapshot(
                    workspace,
                    workspace
                        .panes
                        .get(leaf_id)
                        .expect("pane tab leaf should reference a pane"),
                ),
            ),
            taskers_domain::PaneTabLayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => PaneTabLayoutSnapshot::Split {
                axis: SplitAxis::from_domain(*axis),
                ratio: f32::from(*ratio) / 1000.0,
                first: Box::new(self.pane_tab_layout_snapshot(workspace, first)),
                second: Box::new(self.pane_tab_layout_snapshot(workspace, second)),
            },
        }
    }

    fn live_pane_snapshot(
        &self,
        workspace: &Workspace,
        pane: &taskers_domain::PaneRecord,
    ) -> LivePaneSnapshot {
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
                browser_profile_mode: surface.metadata.browser_profile_mode,
                cwd: normalized_cwd(&surface.metadata),
                attention: surface.attention.into(),
                notification_ring: surface_notification_ring(surface),
                interrupted_agent_resume: surface.interrupted_agent_resume.as_ref().map(|resume| {
                    InterruptedAgentResumeSnapshot {
                        title: resume.title.clone(),
                        command: resume.command.clone(),
                    }
                }),
            })
            .collect::<Vec<_>>();
        LivePaneSnapshot {
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
            profile_mode: surface.metadata.browser_profile_mode,
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
                    let SurfaceMountSpec::Browser(BrowserMountSpec { url, profile_mode }) = mount
                    else {
                        continue;
                    };
                    catalog.push(BrowserSurfaceCatalogEntry {
                        workspace_id: *workspace_id,
                        pane_id: pane.id,
                        surface_id: surface.id,
                        url,
                        profile_mode,
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
                    let descriptor = self
                        .app_state
                        .surface_descriptor_for_surface(*workspace_id, pane.id, surface.id)
                        .unwrap_or_else(|_| fallback_surface_descriptor(surface));
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
                let layout = window.active_layout()?;
                Some(self.collect_surface_plans(
                    workspace_id,
                    workspace,
                    layout,
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
            taskers_domain::LayoutNode::Leaf { leaf_id } => workspace
                .pane_containers
                .get(leaf_id)
                .and_then(|pane_container| pane_container.active_tab_record())
                .map(|pane_tab| {
                    self.collect_pane_tab_surface_plans(
                        workspace_id,
                        workspace,
                        &pane_tab.layout,
                        pane_container_content_frame(frame, self.metrics),
                    )
                })
                .unwrap_or_default(),
            taskers_domain::LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = SplitAxis::from_domain(*axis);
                let (first_frame, second_frame) =
                    split_frame(frame, axis, *ratio, self.metrics.split_gap);
                if should_render_split_as_collapsed(frame, axis, *ratio, self.metrics.split_gap) {
                    let active_pane = workspace.active_pane;
                    let render_first = layout_node_contains_pane(workspace, first, active_pane)
                        || !layout_node_contains_pane(workspace, second, active_pane);
                    return if render_first {
                        self.collect_surface_plans(workspace_id, workspace, first, frame)
                    } else {
                        self.collect_surface_plans(workspace_id, workspace, second, frame)
                    };
                }
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

    fn collect_pane_tab_surface_plans(
        &self,
        workspace_id: WorkspaceId,
        workspace: &Workspace,
        node: &PaneTabLayoutNode,
        frame: Frame,
    ) -> Vec<PortalSurfacePlan> {
        match node {
            PaneTabLayoutNode::Leaf { leaf_id } => workspace
                .panes
                .get(leaf_id)
                .and_then(|pane| {
                    let active_surface = pane.active_surface()?;
                    Some(PortalSurfacePlan {
                        pane_id: pane.id,
                        surface_id: active_surface.id,
                        active: workspace.active_pane == pane.id,
                        notification_ring: pane_notification_ring(pane),
                        pane_frame: frame,
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
            PaneTabLayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = SplitAxis::from_domain(*axis);
                let (first_frame, second_frame) =
                    split_frame(frame, axis, *ratio, self.metrics.split_gap);
                if should_render_split_as_collapsed(frame, axis, *ratio, self.metrics.split_gap) {
                    let active_pane = workspace.active_pane;
                    let render_first = first.contains(active_pane) || !second.contains(active_pane);
                    return if render_first {
                        self.collect_pane_tab_surface_plans(workspace_id, workspace, first, frame)
                    } else {
                        self.collect_pane_tab_surface_plans(workspace_id, workspace, second, frame)
                    };
                }
                let mut plans = self.collect_pane_tab_surface_plans(
                    workspace_id,
                    workspace,
                    first,
                    first_frame,
                );
                plans.extend(self.collect_pane_tab_surface_plans(
                    workspace_id,
                    workspace,
                    second,
                    second_frame,
                ));
                plans
            }
        }
    }

    fn resize_handles_snapshot(
        &self,
        workspace_id: WorkspaceId,
        workspace: &Workspace,
        window_frames: &BTreeMap<WorkspaceWindowId, (WorkspaceColumnId, Frame)>,
    ) -> Vec<ResizeHandleSnapshot> {
        let mut handles = Vec::new();
        let workspace_window_gap = self.ui.workspace_window_gap;
        let ordered_columns = workspace.columns.values().collect::<Vec<_>>();
        let column_widths = ordered_columns
            .iter()
            .filter_map(|column| {
                column.window_order.iter().find_map(|window_id| {
                    window_frames
                        .get(window_id)
                        .map(|(_, frame)| (column.id, frame.width))
                })
            })
            .collect::<Vec<_>>();

        for (column_index, column) in ordered_columns.iter().enumerate() {
            let window_ids = column
                .window_order
                .iter()
                .copied()
                .filter(|window_id| workspace.windows.contains_key(window_id))
                .collect::<Vec<_>>();
            let window_heights = window_ids
                .iter()
                .filter_map(|window_id| {
                    window_frames
                        .get(window_id)
                        .map(|(_, frame)| (*window_id, frame.height))
                })
                .collect::<Vec<_>>();
            let is_leftmost_column = column_index == 0;
            let has_right_neighbor = column_index + 1 < ordered_columns.len();
            let is_rightmost_column = !has_right_neighbor;

            for (window_index, window_id) in window_ids.iter().enumerate() {
                let Some(window) = workspace.windows.get(window_id) else {
                    continue;
                };
                let Some((_, frame)) = window_frames.get(window_id) else {
                    continue;
                };
                let has_bottom_neighbor = window_index + 1 < window_ids.len();

                if is_leftmost_column {
                    handles.push(ResizeHandleSnapshot {
                        id: format!("workspace-column-outer-left-{}-{}", column.id, window.id),
                        frame: workspace_window_outer_edge_handle_frame(
                            *frame,
                            WorkspaceOuterEdge::Left,
                        ),
                        cursor: ResizeHandleCursor::EastWest,
                        target: ResizeHandleTarget::WorkspaceColumnOuterEdge {
                            workspace_id,
                            column_widths: column_widths.clone(),
                            column_index,
                            edge: WorkspaceOuterEdge::Left,
                        },
                    });
                }

                if has_right_neighbor {
                    handles.push(ResizeHandleSnapshot {
                        id: format!("workspace-column-edge-{}-{}", column.id, window.id),
                        frame: workspace_window_edge_handle_frame(
                            *frame,
                            true,
                            workspace_window_gap,
                        ),
                        cursor: ResizeHandleCursor::EastWest,
                        target: ResizeHandleTarget::WorkspaceColumnEdge {
                            workspace_id,
                            column_widths: column_widths.clone(),
                            leading_index: column_index,
                        },
                    });
                }

                if is_rightmost_column {
                    handles.push(ResizeHandleSnapshot {
                        id: format!("workspace-column-outer-right-{}-{}", column.id, window.id),
                        frame: workspace_window_outer_edge_handle_frame(
                            *frame,
                            WorkspaceOuterEdge::Right,
                        ),
                        cursor: ResizeHandleCursor::EastWest,
                        target: ResizeHandleTarget::WorkspaceColumnOuterEdge {
                            workspace_id,
                            column_widths: column_widths.clone(),
                            column_index,
                            edge: WorkspaceOuterEdge::Right,
                        },
                    });
                }

                if has_bottom_neighbor {
                    handles.push(ResizeHandleSnapshot {
                        id: format!("workspace-window-bottom-{}", window.id),
                        frame: workspace_window_edge_handle_frame(
                            *frame,
                            false,
                            workspace_window_gap,
                        ),
                        cursor: ResizeHandleCursor::NorthSouth,
                        target: ResizeHandleTarget::WorkspaceWindowBottomEdge {
                            workspace_id,
                            window_heights: window_heights.clone(),
                            upper_index: window_index,
                        },
                    });
                }

                if has_right_neighbor && has_bottom_neighbor {
                    handles.push(ResizeHandleSnapshot {
                        id: format!("workspace-window-corner-{}", window.id),
                        frame: workspace_window_corner_handle_frame(*frame, workspace_window_gap),
                        cursor: ResizeHandleCursor::SouthEast,
                        target: ResizeHandleTarget::WorkspaceWindowCorner {
                            workspace_id,
                            column_widths: column_widths.clone(),
                            leading_index: column_index,
                            window_heights: window_heights.clone(),
                            upper_index: window_index,
                        },
                    });
                }

                let Some(layout) = window.active_layout() else {
                    continue;
                };
                let mut path = Vec::new();
                self.collect_window_split_resize_handles(
                    workspace_id,
                    workspace,
                    window.id,
                    layout,
                    workspace_window_content_frame(*frame, self.metrics),
                    window
                        .active_pane()
                        .expect("workspace window should have an active pane"),
                    &mut path,
                    &mut handles,
                );
            }
        }

        handles
    }

    fn collect_window_split_resize_handles(
        &self,
        workspace_id: WorkspaceId,
        workspace: &Workspace,
        workspace_window_id: WorkspaceWindowId,
        node: &taskers_domain::LayoutNode,
        frame: Frame,
        active_pane: PaneId,
        path: &mut Vec<bool>,
        handles: &mut Vec<ResizeHandleSnapshot>,
    ) {
        match node {
            taskers_domain::LayoutNode::Leaf { leaf_id } => {
                let Some(pane_container) = workspace.pane_containers.get(leaf_id) else {
                    return;
                };
                let Some(pane_tab) = pane_container.active_tab_record() else {
                    return;
                };
                let mut pane_path = Vec::new();
                self.collect_pane_split_resize_handles(
                    workspace_id,
                    pane_container.id,
                    pane_tab.id,
                    &pane_tab.layout,
                    pane_container_content_frame(frame, self.metrics),
                    pane_tab.active_pane,
                    &mut pane_path,
                    handles,
                );
            }
            taskers_domain::LayoutNode::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let axis = SplitAxis::from_domain(*axis);
                let (first_frame, second_frame) =
                    split_frame(frame, axis, *ratio, self.metrics.split_gap);
                if should_render_split_as_collapsed(frame, axis, *ratio, self.metrics.split_gap) {
                    let render_first = layout_node_contains_pane(workspace, first, active_pane)
                        || !layout_node_contains_pane(workspace, second, active_pane);
                    if render_first {
                        path.push(false);
                        self.collect_window_split_resize_handles(
                            workspace_id,
                            workspace,
                            workspace_window_id,
                            first,
                            frame,
                            active_pane,
                            path,
                            handles,
                        );
                        path.pop();
                    } else {
                        path.push(true);
                        self.collect_window_split_resize_handles(
                            workspace_id,
                            workspace,
                            workspace_window_id,
                            second,
                            frame,
                            active_pane,
                            path,
                            handles,
                        );
                        path.pop();
                    }
                    return;
                }

                handles.push(ResizeHandleSnapshot {
                    id: format!(
                        "workspace-window-split-{}-{}",
                        workspace_window_id,
                        resize_path_id(path)
                    ),
                    frame: split_resize_handle_frame(frame, axis, *ratio, self.metrics.split_gap),
                    cursor: split_resize_cursor(axis),
                    target: ResizeHandleTarget::WorkspaceWindowSplit {
                        workspace_id,
                        workspace_window_id,
                        path: path.clone(),
                        axis,
                        parent_frame: frame,
                        initial_ratio: *ratio,
                    },
                });

                path.push(false);
                self.collect_window_split_resize_handles(
                    workspace_id,
                    workspace,
                    workspace_window_id,
                    first,
                    first_frame,
                    active_pane,
                    path,
                    handles,
                );
                path.pop();

                path.push(true);
                self.collect_window_split_resize_handles(
                    workspace_id,
                    workspace,
                    workspace_window_id,
                    second,
                    second_frame,
                    active_pane,
                    path,
                    handles,
                );
                path.pop();
            }
        }
    }

    fn collect_pane_split_resize_handles(
        &self,
        workspace_id: WorkspaceId,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        node: &PaneTabLayoutNode,
        frame: Frame,
        active_pane: PaneId,
        path: &mut Vec<bool>,
        handles: &mut Vec<ResizeHandleSnapshot>,
    ) {
        let PaneTabLayoutNode::Split {
            axis,
            ratio,
            first,
            second,
        } = node
        else {
            return;
        };

        let axis = SplitAxis::from_domain(*axis);
        let (first_frame, second_frame) = split_frame(frame, axis, *ratio, self.metrics.split_gap);
        if should_render_split_as_collapsed(frame, axis, *ratio, self.metrics.split_gap) {
            let render_first = first.contains(active_pane) || !second.contains(active_pane);
            if render_first {
                path.push(false);
                self.collect_pane_split_resize_handles(
                    workspace_id,
                    pane_container_id,
                    pane_tab_id,
                    first,
                    frame,
                    active_pane,
                    path,
                    handles,
                );
                path.pop();
            } else {
                path.push(true);
                self.collect_pane_split_resize_handles(
                    workspace_id,
                    pane_container_id,
                    pane_tab_id,
                    second,
                    frame,
                    active_pane,
                    path,
                    handles,
                );
                path.pop();
            }
            return;
        }

        handles.push(ResizeHandleSnapshot {
            id: format!(
                "pane-tab-split-{}-{}-{}",
                pane_container_id,
                pane_tab_id,
                resize_path_id(path)
            ),
            frame: split_resize_handle_frame(frame, axis, *ratio, self.metrics.split_gap),
            cursor: split_resize_cursor(axis),
            target: ResizeHandleTarget::PaneTabSplit {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path: path.clone(),
                axis,
                parent_frame: frame,
                initial_ratio: *ratio,
            },
        });

        path.push(false);
        self.collect_pane_split_resize_handles(
            workspace_id,
            pane_container_id,
            pane_tab_id,
            first,
            first_frame,
            active_pane,
            path,
            handles,
        );
        path.pop();

        path.push(true);
        self.collect_pane_split_resize_handles(
            workspace_id,
            pane_container_id,
            pane_tab_id,
            second,
            second_frame,
            active_pane,
            path,
            handles,
        );
        path.pop();
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
        let _ = self.bootstrap_active_workspace_top_level_extents_if_needed();
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
                self.clear_vcs_surface_target(surface_id);
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
                self.ui.resize_preview = None;
                self.ui.section = section;
                self.bump_local_revision();
                true
            }
            ShellAction::ToggleOverview => {
                self.ui.resize_preview = None;
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
            ShellAction::CreateWorkspaceWindowTab { window_id } => {
                self.create_workspace_window_tab(window_id)
            }
            ShellAction::FocusWorkspaceWindowTab { window_id, tab_id } => {
                self.focus_workspace_window_tab(window_id, tab_id)
            }
            ShellAction::MoveWorkspaceWindowTab {
                window_id,
                tab_id,
                target_index,
            } => self.move_workspace_window_tab(window_id, tab_id, target_index),
            ShellAction::TransferWorkspaceWindowTab {
                source_window_id,
                tab_id,
                target_window_id,
                target_index,
            } => self.transfer_workspace_window_tab(
                source_window_id,
                tab_id,
                target_window_id,
                target_index,
            ),
            ShellAction::ExtractWorkspaceWindowTab {
                source_window_id,
                tab_id,
                target,
            } => self.extract_workspace_window_tab(source_window_id, tab_id, target),
            ShellAction::CloseWorkspaceWindowTab { window_id, tab_id } => {
                self.close_workspace_window_tab(window_id, tab_id)
            }
            ShellAction::CreatePaneTab {
                pane_container_id,
                kind,
            } => self.create_pane_tab(pane_container_id, kind),
            ShellAction::FocusPaneTab {
                pane_container_id,
                pane_tab_id,
            } => self.focus_pane_tab(pane_container_id, pane_tab_id),
            ShellAction::MovePaneTab {
                pane_container_id,
                pane_tab_id,
                target_index,
            } => self.move_pane_tab(pane_container_id, pane_tab_id, target_index),
            ShellAction::TransferPaneTab {
                source_pane_container_id,
                pane_tab_id,
                target_pane_container_id,
                target_index,
            } => self.transfer_pane_tab(
                source_pane_container_id,
                pane_tab_id,
                target_pane_container_id,
                target_index,
            ),
            ShellAction::ClosePaneTab {
                pane_container_id,
                pane_tab_id,
            } => self.close_pane_tab(pane_container_id, pane_tab_id),
            ShellAction::ScrollViewport { dx, dy } => self.scroll_viewport_by(dx, dy),
            ShellAction::SplitBrowser {
                pane_id,
                profile_mode,
            } => self.split_with_kind_axis(
                pane_id,
                PaneKind::Browser,
                DomainSplitAxis::Horizontal,
                profile_mode,
            ),
            ShellAction::SplitTerminal { pane_id } => self.split_with_kind_axis(
                pane_id,
                PaneKind::Terminal,
                DomainSplitAxis::Horizontal,
                BrowserProfileMode::PersistentDefault,
            ),
            ShellAction::AddBrowserSurface {
                pane_id,
                profile_mode,
            } => self.add_surface_to_pane(pane_id, PaneKind::Browser, profile_mode),
            ShellAction::AddTerminalSurface { pane_id } => self.add_surface_to_pane(
                pane_id,
                PaneKind::Terminal,
                BrowserProfileMode::PersistentDefault,
            ),
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
            ShellAction::BeginWindowTabDrag { window_id, tab_id } => {
                self.begin_window_tab_drag(window_id, tab_id)
            }
            ShellAction::BeginPaneTabDrag {
                pane_container_id,
                pane_tab_id,
            } => self.begin_pane_tab_drag(pane_container_id, pane_tab_id),
            ShellAction::BeginSurfaceDrag {
                workspace_id,
                pane_id,
                surface_id,
            } => self.begin_surface_drag(workspace_id, pane_id, surface_id),
            ShellAction::PreviewSurfaceDragWorkspace { workspace_id } => {
                self.preview_surface_drag_workspace(workspace_id)
            }
            ShellAction::PreviewResize { preview } => self.preview_resize(preview),
            ShellAction::CommitResizePreview => self.commit_resize_preview(),
            ShellAction::CancelResizePreview => self.cancel_resize_preview(),
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
            ShellAction::ClearBrowserData { surface_id } => {
                self.queue_host_command(HostCommand::BrowserClearData { surface_id })
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
            ShellAction::DismissSurfaceAlert {
                workspace_id,
                pane_id,
                surface_id,
            } => self.dismiss_surface_alert(workspace_id, pane_id, surface_id),
            ShellAction::ResumeInterruptedAgent {
                workspace_id,
                pane_id,
                surface_id,
            } => self.resume_interrupted_agent(workspace_id, pane_id, surface_id),
            ShellAction::DismissInterruptedAgentResume {
                workspace_id,
                pane_id,
                surface_id,
            } => self.dismiss_interrupted_agent_resume(workspace_id, pane_id, surface_id),
            ShellAction::ToggleVcsPanel => self.toggle_vcs_panel(),
            ShellAction::RefreshVcsPanel => self.refresh_vcs_panel(),
            ShellAction::ShowVcsDiff { path } => self.show_vcs_diff(path),
            ShellAction::RunVcsCommand { command } => self.run_vcs_command(command),
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
            ShellAction::SetConfiguredShell { shell } => {
                let shell = normalize_configured_shell(shell.as_deref());
                if self.ui.configured_shell == shell {
                    return false;
                }
                self.ui.configured_shell = shell;
                self.bump_local_revision();
                true
            }
            ShellAction::SetEmbeddedTerminalTextSetting { key, value } => {
                let normalized = value.trim().to_string();
                let field = match key {
                    EmbeddedTerminalTextSettingKey::Theme => {
                        &mut self.ui.embedded_terminal_settings.theme
                    }
                    EmbeddedTerminalTextSettingKey::FontFamily => {
                        &mut self.ui.embedded_terminal_settings.font_family
                    }
                    EmbeddedTerminalTextSettingKey::FontSize => {
                        &mut self.ui.embedded_terminal_settings.font_size
                    }
                    EmbeddedTerminalTextSettingKey::WindowPaddingX => {
                        &mut self.ui.embedded_terminal_settings.window_padding_x
                    }
                    EmbeddedTerminalTextSettingKey::WindowPaddingY => {
                        &mut self.ui.embedded_terminal_settings.window_padding_y
                    }
                    EmbeddedTerminalTextSettingKey::CursorStyle => {
                        &mut self.ui.embedded_terminal_settings.cursor_style
                    }
                    EmbeddedTerminalTextSettingKey::ScrollbackLimit => {
                        &mut self.ui.embedded_terminal_settings.scrollback_limit
                    }
                    EmbeddedTerminalTextSettingKey::BackgroundOpacity => {
                        &mut self.ui.embedded_terminal_settings.background_opacity
                    }
                };
                if *field == normalized {
                    return false;
                }
                *field = normalized;
                self.bump_local_revision();
                true
            }
            ShellAction::SetEmbeddedTerminalBoolSetting { key, value } => {
                let field = match key {
                    EmbeddedTerminalBoolSettingKey::CursorStyleBlink => {
                        &mut self.ui.embedded_terminal_settings.cursor_style_blink
                    }
                    EmbeddedTerminalBoolSettingKey::BackgroundOpacityCells => {
                        &mut self.ui.embedded_terminal_settings.background_opacity_cells
                    }
                };
                if *field == value {
                    return false;
                }
                *field = value;
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
            ShellAction::SetOverviewLiveSurfaces { enabled } => {
                if self.ui.render_live_surfaces_in_overview == enabled {
                    return false;
                }
                self.ui.render_live_surfaces_in_overview = enabled;
                self.bump_local_revision();
                true
            }
            ShellAction::SetWorkspaceWindowGap { gap } => {
                let gap = clamp_workspace_window_gap(gap);
                if self.ui.workspace_window_gap == gap {
                    return false;
                }
                self.ui.workspace_window_gap = gap;
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
            ShortcutAction::CloseTerminal => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| {
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
                },
            ),
            ShortcutAction::OpenBrowserSplit => self.run_standard_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(
                    None,
                    PaneKind::Browser,
                    DomainSplitAxis::Horizontal,
                    BrowserProfileMode::PersistentDefault,
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
            ShortcutAction::FocusLeft => {
                if self.ui.overview_mode {
                    self.run_workspace_shortcut(true, |core, _| {
                        Some(core.focus_active_workspace_window(Direction::Left))
                    })
                } else {
                    self.run_standard_workspace_shortcut(|core, workspace_id| {
                        Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                            workspace_id,
                            direction: Direction::Left,
                        }))
                    })
                }
            }
            ShortcutAction::FocusRight => {
                if self.ui.overview_mode {
                    self.run_workspace_shortcut(true, |core, _| {
                        Some(core.focus_active_workspace_window(Direction::Right))
                    })
                } else {
                    self.run_standard_workspace_shortcut(|core, workspace_id| {
                        Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                            workspace_id,
                            direction: Direction::Right,
                        }))
                    })
                }
            }
            ShortcutAction::FocusUp => {
                if self.ui.overview_mode {
                    self.run_workspace_shortcut(true, |core, _| {
                        Some(core.focus_active_workspace_window(Direction::Up))
                    })
                } else {
                    self.run_standard_workspace_shortcut(|core, workspace_id| {
                        Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                            workspace_id,
                            direction: Direction::Up,
                        }))
                    })
                }
            }
            ShortcutAction::FocusDown => {
                if self.ui.overview_mode {
                    self.run_workspace_shortcut(true, |core, _| {
                        Some(core.focus_active_workspace_window(Direction::Down))
                    })
                } else {
                    self.run_standard_workspace_shortcut(|core, workspace_id| {
                        Some(core.dispatch_control(ControlCommand::FocusPaneDirection {
                            workspace_id,
                            direction: Direction::Down,
                        }))
                    })
                }
            }
            ShortcutAction::NewWindowLeft => self.run_workspace_shortcut(true, |core, _| {
                Some(core.create_workspace_window_from_active_terminal(WorkspaceDirection::Left))
            }),
            ShortcutAction::NewWindowRight => self.run_workspace_shortcut(true, |core, _| {
                Some(core.create_workspace_window_from_active_terminal(WorkspaceDirection::Right))
            }),
            ShortcutAction::NewWindowUp => self.run_workspace_shortcut(true, |core, _| {
                Some(core.create_workspace_window_from_active_terminal(WorkspaceDirection::Up))
            }),
            ShortcutAction::NewWindowDown => self.run_workspace_shortcut(true, |core, _| {
                Some(core.create_workspace_window_from_active_terminal(WorkspaceDirection::Down))
            }),
            ShortcutAction::MoveWindowLeft
            | ShortcutAction::MoveWindowRight
            | ShortcutAction::MoveWindowUp
            | ShortcutAction::MoveWindowDown => {
                self.run_workspace_shortcut(shortcut_preserves_overview(action), |core, _| {
                    let direction = match action {
                        ShortcutAction::MoveWindowLeft => Direction::Left,
                        ShortcutAction::MoveWindowRight => Direction::Right,
                        ShortcutAction::MoveWindowUp => Direction::Up,
                        ShortcutAction::MoveWindowDown => Direction::Down,
                        _ => unreachable!("move action already matched"),
                    };
                    Some(core.move_active_workspace_window(direction))
                })
            }
            ShortcutAction::ResizeWindowLeft => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Left,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                },
            ),
            ShortcutAction::ResizeWindowRight => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Right,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                },
            ),
            ShortcutAction::ResizeWindowUp => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Up,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                },
            ),
            ShortcutAction::ResizeWindowDown => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| {
                    Some(core.dispatch_control(ControlCommand::ResizeActiveWindow {
                        workspace_id,
                        direction: Direction::Down,
                        amount: KEYBOARD_RESIZE_STEP,
                    }))
                },
            ),
            ShortcutAction::ResizeSplitLeft => {
                self.run_standard_workspace_shortcut(|core, workspace_id| {
                    Some(core.resize_active_terminal_horizontally(workspace_id, Direction::Left))
                })
            }
            ShortcutAction::ResizeSplitRight => {
                self.run_standard_workspace_shortcut(|core, workspace_id| {
                    Some(core.resize_active_terminal_horizontally(workspace_id, Direction::Right))
                })
            }
            ShortcutAction::ResizeSplitUp => {
                self.run_standard_workspace_shortcut(|core, workspace_id| {
                    Some(
                        core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                            workspace_id,
                            direction: Direction::Up,
                            amount: KEYBOARD_RESIZE_STEP,
                        }),
                    )
                })
            }
            ShortcutAction::ResizeSplitDown => {
                self.run_standard_workspace_shortcut(|core, workspace_id| {
                    Some(
                        core.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                            workspace_id,
                            direction: Direction::Down,
                            amount: KEYBOARD_RESIZE_STEP,
                        }),
                    )
                })
            }
            ShortcutAction::FitTerminalToViewport => self.run_workspace_shortcut(
                shortcut_preserves_overview(action),
                |core, workspace_id| Some(core.fit_active_terminal_to_viewport(workspace_id)),
            ),
            ShortcutAction::SplitRight => self.run_standard_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(
                    None,
                    PaneKind::Terminal,
                    DomainSplitAxis::Horizontal,
                    BrowserProfileMode::PersistentDefault,
                ))
            }),
            ShortcutAction::SplitDown => self.run_standard_workspace_shortcut(|core, _| {
                Some(core.split_with_kind_axis(
                    None,
                    PaneKind::Terminal,
                    DomainSplitAxis::Vertical,
                    BrowserProfileMode::PersistentDefault,
                ))
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
        changed |= self.sync_terminal_focus_for_workspace(workspace_id);
        if self.ui.vcs_panel_visible {
            changed |= self.refresh_vcs_panel();
        }
        changed |= self.bootstrap_active_workspace_top_level_extents_if_needed();
        changed
    }

    fn create_workspace(&mut self) -> bool {
        let label = next_workspace_label(&self.app_state.snapshot_model());
        let changed = self.dispatch_control(ControlCommand::CreateWorkspace { label });
        self.bootstrap_active_workspace_top_level_extents_if_needed() || changed
    }

    fn bootstrap_active_workspace_top_level_extents_if_needed(&mut self) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        if workspace.top_level_extents_initialized {
            return false;
        }

        let viewport = self.workspace_viewport_frame(attention_panel_visible(&model));
        let column_width = (viewport.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2)
            .max(MIN_WORKSPACE_WINDOW_WIDTH);
        let window_height = viewport.height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
        self.dispatch_control(ControlCommand::BootstrapWorkspaceTopLevelExtents {
            workspace_id,
            column_width,
            window_height,
        })
    }

    fn create_workspace_window(&mut self, direction: WorkspaceDirection) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let changed = self.dispatch_control(ControlCommand::CreateWorkspaceWindow {
            workspace_id,
            direction: direction.to_domain(),
            preferred_column_width: None,
            preferred_window_height: None,
        });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn create_workspace_window_from_active_terminal(
        &mut self,
        direction: WorkspaceDirection,
    ) -> bool {
        let snapshot = self.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let Some(active_window) = snapshot
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == snapshot.current_workspace.active_window_id)
        else {
            return false;
        };
        let gap = self.ui.workspace_window_gap.max(0);
        let (preferred_column_width, preferred_window_height) = match direction {
            WorkspaceDirection::Left | WorkspaceDirection::Right => (
                Some(
                    ((active_window.frame.width - gap).max(2) / 2).max(MIN_WORKSPACE_WINDOW_WIDTH),
                ),
                None,
            ),
            WorkspaceDirection::Up | WorkspaceDirection::Down => (
                None,
                Some(
                    ((active_window.frame.height - gap).max(2) / 2)
                        .max(MIN_WORKSPACE_WINDOW_HEIGHT),
                ),
            ),
        };

        let mut changed = self.dispatch_control(ControlCommand::CreateWorkspaceWindow {
            workspace_id,
            direction: direction.to_domain(),
            preferred_column_width,
            preferred_window_height,
        });
        match direction {
            WorkspaceDirection::Left | WorkspaceDirection::Right => {
                if let Some(width) = preferred_column_width {
                    changed |= self.dispatch_control(ControlCommand::SetWorkspaceColumnWidth {
                        workspace_id,
                        workspace_column_id: active_window.column_id,
                        width,
                    });
                }
            }
            WorkspaceDirection::Up | WorkspaceDirection::Down => {
                if let Some(height) = preferred_window_height {
                    changed |= self.dispatch_control(ControlCommand::SetWorkspaceWindowHeight {
                        workspace_id,
                        workspace_window_id: active_window.id,
                        height,
                    });
                }
            }
        }
        changed |= self.ensure_active_window_visible();
        changed
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

    fn create_workspace_window_tab(&mut self, window_id: WorkspaceWindowId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::CreateWorkspaceWindowTab {
            workspace_id,
            workspace_window_id: window_id,
        })
    }

    fn focus_workspace_window_tab(
        &mut self,
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::FocusWorkspaceWindowTab {
            workspace_id,
            workspace_window_id: window_id,
            workspace_window_tab_id: tab_id,
        })
    }

    fn move_workspace_window_tab(
        &mut self,
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target_index: usize,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::MoveWorkspaceWindowTab {
            workspace_id,
            workspace_window_id: window_id,
            workspace_window_tab_id: tab_id,
            to_index: target_index,
        })
    }

    fn transfer_workspace_window_tab(
        &mut self,
        source_window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target_window_id: WorkspaceWindowId,
        target_index: usize,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let changed = self.dispatch_control(ControlCommand::TransferWorkspaceWindowTab {
            workspace_id,
            source_workspace_window_id: source_window_id,
            workspace_window_tab_id: tab_id,
            target_workspace_window_id: target_window_id,
            to_index: target_index,
        });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn extract_workspace_window_tab(
        &mut self,
        source_window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
        target: WorkspaceWindowMoveTarget,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let changed = self.dispatch_control(ControlCommand::ExtractWorkspaceWindowTab {
            workspace_id,
            source_workspace_window_id: source_window_id,
            workspace_window_tab_id: tab_id,
            target,
        });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn close_workspace_window_tab(
        &mut self,
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        self.dispatch_control(ControlCommand::CloseWorkspaceWindowTab {
            workspace_id,
            workspace_window_id: window_id,
            workspace_window_tab_id: tab_id,
        })
    }

    fn create_pane_tab(&mut self, pane_container_id: PaneContainerId, kind: PaneKind) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = self.resolve_workspace_pane_container(&model, pane_container_id)
        else {
            return false;
        };
        self.dispatch_control(ControlCommand::CreatePaneTab {
            workspace_id,
            pane_container_id,
            kind,
        })
    }

    fn focus_pane_tab(
        &mut self,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = self.resolve_workspace_pane_container(&model, pane_container_id)
        else {
            return false;
        };
        self.dispatch_control(ControlCommand::FocusPaneTab {
            workspace_id,
            pane_container_id,
            pane_tab_id,
        })
    }

    fn move_pane_tab(
        &mut self,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        target_index: usize,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = self.resolve_workspace_pane_container(&model, pane_container_id)
        else {
            return false;
        };
        self.dispatch_control(ControlCommand::MovePaneTab {
            workspace_id,
            pane_container_id,
            pane_tab_id,
            to_index: target_index,
        })
    }

    fn transfer_pane_tab(
        &mut self,
        source_pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
        target_pane_container_id: PaneContainerId,
        target_index: usize,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(source_workspace_id) =
            self.resolve_workspace_pane_container(&model, source_pane_container_id)
        else {
            return false;
        };
        let Some(target_workspace_id) =
            self.resolve_workspace_pane_container(&model, target_pane_container_id)
        else {
            return false;
        };
        if source_workspace_id != target_workspace_id {
            return false;
        }
        let changed = self.dispatch_control(ControlCommand::TransferPaneTab {
            workspace_id: source_workspace_id,
            source_pane_container_id,
            pane_tab_id,
            target_pane_container_id,
            to_index: target_index,
        });
        if changed {
            return self.ensure_active_window_visible() || changed;
        }
        false
    }

    fn close_pane_tab(
        &mut self,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = self.resolve_workspace_pane_container(&model, pane_container_id)
        else {
            return false;
        };
        self.dispatch_control(ControlCommand::ClosePaneTab {
            workspace_id,
            pane_container_id,
            pane_tab_id,
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
            self.ui.workspace_window_gap,
            workspace.viewport.clone(),
        );
        let next_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            self.ui.workspace_window_gap,
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
        browser_profile_mode: BrowserProfileMode,
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

        let is_browser = matches!(kind, PaneKind::Browser);
        let created = self.dispatch_control(ControlCommand::CreateSurface {
            workspace_id,
            pane_id: new_pane_id,
            kind,
            browser_profile_mode: is_browser.then_some(browser_profile_mode),
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

    fn add_surface_to_pane(
        &mut self,
        pane_id: Option<PaneId>,
        kind: PaneKind,
        browser_profile_mode: BrowserProfileMode,
    ) -> bool {
        let Some((workspace_id, target_pane_id)) = self.resolve_target_pane(pane_id) else {
            return false;
        };
        let is_browser = matches!(kind, PaneKind::Browser);
        self.dispatch_control(ControlCommand::CreateSurface {
            workspace_id,
            pane_id: target_pane_id,
            kind,
            browser_profile_mode: is_browser.then_some(browser_profile_mode),
        })
    }

    fn focus_pane_by_id(&mut self, pane_id: PaneId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some((workspace_id, _)) = self.resolve_workspace_pane(&model, pane_id) else {
            return false;
        };
        let starting_app_revision = self.app_state.revision();
        let mut changed = false;
        if model.active_workspace_id() != Some(workspace_id) {
            changed |= self.dispatch_control(ControlCommand::SwitchWorkspace {
                window_id: None,
                workspace_id,
            });
        }
        let _ = self.dispatch_control_with_response(ControlCommand::FocusPane {
            workspace_id,
            pane_id,
        });
        changed |= self.app_state.revision() != starting_app_revision;
        changed |= self.sync_terminal_focus_for_workspace(workspace_id);
        if self.ui.vcs_panel_visible {
            changed |= self.refresh_vcs_panel();
        }
        changed
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
        let mut changed = self.dispatch_control(ControlCommand::FocusSurface {
            workspace_id,
            pane_id,
            surface_id,
        });
        changed |= self.record_terminal_focus(workspace_id, surface_id);
        if self.ui.vcs_panel_visible {
            changed |= self.refresh_vcs_panel();
        }
        changed
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

    fn dismiss_surface_alert(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> bool {
        self.dispatch_control(ControlCommand::DismissSurfaceAlert {
            workspace_id,
            pane_id,
            surface_id,
        })
    }

    fn resume_interrupted_agent(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> bool {
        let command = self
            .app_state
            .snapshot_model()
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.surfaces.get(&surface_id))
            .and_then(|surface| surface.interrupted_agent_resume.as_ref())
            .map(|resume| resume.command.clone());
        let Some(command) = command else {
            return false;
        };

        let mut changed = self.dispatch_control(ControlCommand::DismissInterruptedAgentResume {
            workspace_id,
            pane_id,
            surface_id,
        });
        changed |= self.queue_host_command(HostCommand::TerminalSendText {
            surface_id,
            text: format!("{command}\n"),
        });
        changed
    }

    fn dismiss_interrupted_agent_resume(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> bool {
        self.dispatch_control(ControlCommand::DismissInterruptedAgentResume {
            workspace_id,
            pane_id,
            surface_id,
        })
    }

    fn open_activity(&mut self, activity_id: ActivityId) -> bool {
        self.dispatch_control(ControlCommand::OpenNotification {
            window_id: None,
            notification_id: activity_id.notification_id,
        })
    }

    fn update_surface_metadata(&mut self, surface_id: SurfaceId, patch: PaneMetadataPatch) -> bool {
        let mut changed =
            self.dispatch_control(ControlCommand::UpdateSurfaceMetadata { surface_id, patch });
        let model = self.app_state.snapshot_model();
        let should_refresh_vcs = self.ui.vcs_panel_visible
            && model
                .active_workspace_id()
                .and_then(|workspace_id| self.vcs_target_surface_id(&model, workspace_id))
                == Some(surface_id);
        if should_refresh_vcs {
            changed |= self.refresh_vcs_panel();
        }
        changed
    }

    fn toggle_vcs_panel(&mut self) -> bool {
        self.ui.vcs_panel_visible = !self.ui.vcs_panel_visible;
        if !self.ui.vcs_panel_visible {
            self.bump_local_revision();
            return true;
        }
        let mut changed = self.refresh_vcs_panel();
        if !changed {
            self.bump_local_revision();
            changed = true;
        }
        changed
    }

    fn refresh_vcs_panel(&mut self) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace_id) = model.active_workspace_id() else {
            return false;
        };
        let Some(surface_id) = self.vcs_target_surface_id(&model, workspace_id) else {
            let changed =
                self.ui.vcs_snapshot.take().is_some() || self.ui.vcs_error.take().is_some();
            if changed {
                self.bump_local_revision();
            }
            return changed;
        };
        let response = self.dispatch_control_with_response(ControlCommand::Vcs {
            vcs_command: VcsCommand::Refresh {
                surface_id,
                diff_path: self.ui.vcs_diff_path.clone(),
            },
        });
        self.store_vcs_response(response)
    }

    fn show_vcs_diff(&mut self, path: Option<String>) -> bool {
        if self.ui.vcs_diff_path == path {
            return false;
        }
        self.ui.vcs_diff_path = path;
        self.refresh_vcs_panel()
    }

    fn run_vcs_command(&mut self, command: VcsCommand) -> bool {
        let response = self.dispatch_control_with_response(ControlCommand::Vcs {
            vcs_command: command,
        });
        self.store_vcs_response(response)
    }

    fn store_vcs_response(&mut self, response: Option<ControlResponse>) -> bool {
        let previous_snapshot = self.ui.vcs_snapshot.clone();
        let previous_error = self.ui.vcs_error.clone();
        match response {
            Some(ControlResponse::Vcs { result }) => self.apply_vcs_result(result),
            Some(_) => {
                self.ui.vcs_error = Some("unexpected VCS response".into());
            }
            None => {
                self.ui.vcs_error = Some("VCS request failed".into());
            }
        }
        let changed =
            self.ui.vcs_snapshot != previous_snapshot || self.ui.vcs_error != previous_error;
        if changed {
            self.bump_local_revision();
        }
        changed
    }

    fn apply_vcs_result(&mut self, result: VcsCommandResult) {
        self.ui.vcs_error = None;
        if let Some(snapshot) = result.snapshot {
            self.ui.vcs_snapshot = Some(snapshot);
        } else {
            self.ui.vcs_snapshot = None;
            self.ui.vcs_diff_path = None;
        }
        if let Some(message) = result.message {
            self.ui.vcs_error = Some(message);
        }
    }

    fn sync_terminal_focus_for_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let Some(surface_id) = workspace
            .panes
            .get(&workspace.active_pane)
            .and_then(|pane| pane.active_surface())
            .filter(|surface| surface.kind == PaneKind::Terminal)
            .map(|surface| surface.id)
        else {
            return false;
        };
        self.record_terminal_focus(workspace_id, surface_id)
    }

    fn record_terminal_focus(&mut self, workspace_id: WorkspaceId, surface_id: SurfaceId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let is_terminal_surface = workspace
            .panes
            .values()
            .flat_map(|pane| pane.surfaces.values())
            .any(|surface| surface.id == surface_id && surface.kind == PaneKind::Terminal);
        if !is_terminal_surface {
            return false;
        }
        let _previous = self
            .ui
            .last_terminal_surface_by_workspace
            .insert(workspace_id, surface_id);
        let mut changed = false;
        if self.app_state.snapshot_model().active_workspace_id() == Some(workspace_id) {
            changed = self.ui.vcs_diff_path.take().is_some();
        }
        changed
    }

    fn clear_vcs_surface_target(&mut self, surface_id: SurfaceId) {
        self.ui
            .last_terminal_surface_by_workspace
            .retain(|_, tracked_surface_id| *tracked_surface_id != surface_id);
        if self
            .ui
            .vcs_snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.surface_id == surface_id)
        {
            self.ui.vcs_snapshot = None;
            self.ui.vcs_diff_path = None;
        }
    }

    fn vcs_target_surface_id(
        &self,
        model: &AppModel,
        workspace_id: WorkspaceId,
    ) -> Option<SurfaceId> {
        let workspace = model.workspaces.get(&workspace_id)?;
        if let Some(surface_id) = self
            .ui
            .last_terminal_surface_by_workspace
            .get(&workspace_id)
            .copied()
            .filter(|surface_id| {
                workspace
                    .panes
                    .values()
                    .flat_map(|pane| pane.surfaces.values())
                    .any(|surface| surface.id == *surface_id && surface.kind == PaneKind::Terminal)
            })
        {
            return Some(surface_id);
        }
        workspace
            .panes
            .get(&workspace.active_pane)
            .and_then(|pane| pane.active_surface())
            .filter(|surface| surface.kind == PaneKind::Terminal)
            .map(|surface| surface.id)
            .or_else(|| {
                workspace
                    .panes
                    .values()
                    .flat_map(|pane| pane.surfaces.values())
                    .find(|surface| surface.kind == PaneKind::Terminal)
                    .map(|surface| surface.id)
            })
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

    fn focus_active_workspace_window(&mut self, direction: Direction) -> bool {
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

        let target_window_id = match direction {
            Direction::Left => active_column_index
                .checked_sub(1)
                .and_then(|index| workspace.columns.get_index(index))
                .and_then(|(_, column)| column.window_order.first())
                .copied(),
            Direction::Right => workspace
                .columns
                .get_index(active_column_index + 1)
                .and_then(|(_, column)| column.window_order.first())
                .copied(),
            Direction::Up => workspace
                .columns
                .get(&active_column_id)
                .and_then(|column| {
                    active_window_index
                        .checked_sub(1)
                        .and_then(|index| column.window_order.get(index))
                })
                .copied(),
            Direction::Down => workspace
                .columns
                .get(&active_column_id)
                .and_then(|column| column.window_order.get(active_window_index + 1))
                .copied(),
        };

        let Some(target_window_id) = target_window_id else {
            return false;
        };
        self.focus_workspace_window(target_window_id)
    }

    fn run_workspace_shortcut(
        &mut self,
        preserve_overview: bool,
        handler: impl FnOnce(&mut Self, WorkspaceId) -> Option<bool>,
    ) -> bool {
        let Some(workspace_id) = self.prepare_workspace_interaction(preserve_overview) else {
            return false;
        };
        let Some(mut changed) = handler(self, workspace_id) else {
            return false;
        };
        changed |= self.ensure_active_window_visible();
        changed
    }

    fn run_standard_workspace_shortcut(
        &mut self,
        handler: impl FnOnce(&mut Self, WorkspaceId) -> Option<bool>,
    ) -> bool {
        self.run_workspace_shortcut(false, handler)
    }

    fn active_pane_can_resize_split(
        &self,
        workspace_id: WorkspaceId,
        direction: Direction,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let active_pane = workspace.active_pane;
        let Some(mut layout) = workspace
            .pane_containers
            .values()
            .flat_map(|container| container.tabs.values())
            .find(|pane_tab| pane_tab.layout.contains(active_pane))
            .map(|pane_tab| pane_tab.layout.clone())
        else {
            return false;
        };

        let before = layout.clone();
        layout.resize_leaf(active_pane, direction, KEYBOARD_RESIZE_STEP);
        layout != before
    }

    fn resize_active_terminal_horizontally(
        &mut self,
        workspace_id: WorkspaceId,
        direction: Direction,
    ) -> bool {
        if self.active_pane_can_resize_split(workspace_id, direction) {
            return self.dispatch_control(ControlCommand::ResizeActivePaneSplit {
                workspace_id,
                direction,
                amount: KEYBOARD_RESIZE_STEP,
            });
        }

        self.dispatch_control(ControlCommand::ResizeActiveWindow {
            workspace_id,
            direction,
            amount: KEYBOARD_RESIZE_STEP,
        })
    }

    fn fit_active_terminal_to_viewport(&mut self, workspace_id: WorkspaceId) -> bool {
        let model = self.app_state.snapshot_model();
        let Some(workspace) = model.workspaces.get(&workspace_id) else {
            return false;
        };
        let Some(column_id) = workspace.active_column_id() else {
            return false;
        };
        let Some(window) = workspace.active_window_record() else {
            return false;
        };
        let Some(window_tab) = window.active_tab_record() else {
            return false;
        };
        let active_window_id = window.id;
        let active_container_id = window_tab.active_container;
        let active_pane_id = workspace.active_pane;
        let Some(pane_container) = workspace.pane_containers.get(&active_container_id) else {
            return false;
        };
        let Some(pane_tab_id) = pane_container.tab_for_pane(active_pane_id) else {
            return false;
        };
        let Some(pane_tab) = pane_container.tabs.get(&pane_tab_id) else {
            return false;
        };

        let viewport = self.workspace_viewport_frame(attention_panel_visible(&model));
        let target_column_width = (viewport.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2)
            .max(MIN_WORKSPACE_WINDOW_WIDTH);
        let target_window_height = viewport.height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
        let window_targets = focused_split_ratio_targets(&window_tab.layout, active_container_id);
        let pane_targets = focused_split_ratio_targets(&pane_tab.layout, active_pane_id);

        let mut changed = self.dispatch_control(ControlCommand::SetWorkspaceColumnWidth {
            workspace_id,
            workspace_column_id: column_id,
            width: target_column_width,
        });
        changed |= self.dispatch_control(ControlCommand::SetWorkspaceWindowHeight {
            workspace_id,
            workspace_window_id: active_window_id,
            height: target_window_height,
        });
        for (path, ratio) in window_targets {
            changed |= self.dispatch_control(ControlCommand::SetWindowSplitRatioExact {
                workspace_id,
                workspace_window_id: active_window_id,
                path,
                ratio,
            });
        }
        for (path, ratio) in pane_targets {
            changed |= self.dispatch_control(ControlCommand::SetPaneTabSplitRatioExact {
                workspace_id,
                pane_container_id: active_container_id,
                pane_tab_id,
                path,
                ratio,
            });
        }

        changed
    }

    fn begin_window_drag(&mut self) -> bool {
        let mut changed = false;
        if self.ui.drag_session.is_some() {
            self.ui.drag_session = None;
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

    fn begin_window_tab_drag(
        &mut self,
        window_id: WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    ) -> bool {
        let next =
            DragSessionSnapshot::WindowTab(WindowTabDragSessionSnapshot { window_id, tab_id });
        let mut changed = false;
        if self.ui.drag_session != Some(next) {
            self.ui.drag_session = Some(next);
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::WindowTab {
            self.ui.drag_mode = ShellDragMode::WindowTab;
            changed = true;
        }
        if changed {
            self.bump_local_revision();
        }
        changed
    }

    fn begin_pane_tab_drag(
        &mut self,
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    ) -> bool {
        let model = self.app_state.snapshot_model();
        if self
            .resolve_workspace_pane_container(&model, pane_container_id)
            .is_none()
        {
            return false;
        }
        let next = DragSessionSnapshot::PaneTab(PaneTabDragSessionSnapshot {
            pane_container_id,
            pane_tab_id,
        });
        let mut changed = false;
        if self.ui.drag_session != Some(next) {
            self.ui.drag_session = Some(next);
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::PaneTab {
            self.ui.drag_mode = ShellDragMode::PaneTab;
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
        if self.ui.drag_mode == ShellDragMode::Surface
            && self.ui.drag_session == Some(DragSessionSnapshot::Surface(next))
        {
            return false;
        }
        self.ui.drag_mode = ShellDragMode::Surface;
        self.ui.drag_session = Some(DragSessionSnapshot::Surface(next));
        self.bump_local_revision();
        true
    }

    fn preview_surface_drag_workspace(&mut self, workspace_id: WorkspaceId) -> bool {
        let Some(DragSessionSnapshot::Surface(mut session)) = self.ui.drag_session else {
            return false;
        };
        let mut changed = false;
        if session.preview_workspace_id != workspace_id {
            session.preview_workspace_id = workspace_id;
            self.ui.drag_session = Some(DragSessionSnapshot::Surface(session));
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
        let source_workspace_id = match self.ui.drag_session {
            Some(DragSessionSnapshot::Surface(session)) => Some(session.workspace_id),
            _ => None,
        };
        let mut changed = false;
        if self.ui.drag_session.is_some() {
            self.ui.drag_session = None;
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

    fn prepare_workspace_interaction(&mut self, preserve_overview: bool) -> Option<WorkspaceId> {
        let mut changed = false;
        if self.ui.section != ShellSection::Workspace {
            self.ui.section = ShellSection::Workspace;
            changed = true;
        }
        if self.ui.overview_mode && !preserve_overview {
            self.ui.overview_mode = false;
            changed = true;
        }
        if self.ui.drag_mode != ShellDragMode::None {
            self.ui.drag_mode = ShellDragMode::None;
            changed = true;
        }
        if self.ui.drag_session.is_some() {
            self.ui.drag_session = None;
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
            self.ui.workspace_window_gap,
            workspace.viewport.clone(),
        );
        let Some(active_frame) = workspace_window_placements(
            workspace,
            (viewport_frame.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2).max(1),
            viewport_frame.height,
            self.ui.workspace_window_gap,
        )
        .into_iter()
        .find(|placement| placement.window_id == workspace.active_window)
        .map(|placement| placement.frame) else {
            return false;
        };

        let mut next_viewport = current_viewport;
        let visible_width =
            (viewport_frame.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2).max(1);
        let visible_right = next_viewport.x + visible_width;
        let visible_bottom = next_viewport.y + viewport_frame.height;
        if active_frame.width > visible_width {
            next_viewport.x = active_frame.x;
        } else if active_frame.x < next_viewport.x {
            next_viewport.x = active_frame.x;
        } else if active_frame.right() > visible_right {
            next_viewport.x = active_frame.right().saturating_sub(visible_width);
        }
        if active_frame.height > viewport_frame.height {
            next_viewport.y = active_frame.y;
        } else if active_frame.y < next_viewport.y {
            next_viewport.y = active_frame.y;
        } else if active_frame.bottom() > visible_bottom {
            next_viewport.y = active_frame.bottom() - viewport_frame.height;
        }
        let next_viewport = clamped_workspace_viewport(
            workspace,
            viewport_frame.width,
            viewport_frame.height,
            self.ui.workspace_window_gap,
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

    fn resolve_workspace_pane_container(
        &self,
        model: &AppModel,
        pane_container_id: PaneContainerId,
    ) -> Option<WorkspaceId> {
        model
            .workspaces
            .iter()
            .find_map(|(workspace_id, workspace)| {
                workspace
                    .pane_containers
                    .contains_key(&pane_container_id)
                    .then_some(*workspace_id)
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

fn shortcut_preserves_overview(action: ShortcutAction) -> bool {
    matches!(
        action,
        ShortcutAction::CloseTerminal
            | ShortcutAction::NewWindowLeft
            | ShortcutAction::NewWindowRight
            | ShortcutAction::NewWindowUp
            | ShortcutAction::NewWindowDown
            | ShortcutAction::MoveWindowLeft
            | ShortcutAction::MoveWindowRight
            | ShortcutAction::MoveWindowUp
            | ShortcutAction::MoveWindowDown
            | ShortcutAction::ResizeWindowLeft
            | ShortcutAction::ResizeWindowRight
            | ShortcutAction::ResizeWindowUp
            | ShortcutAction::ResizeWindowDown
            | ShortcutAction::FitTerminalToViewport
    )
}

fn apply_resize_preview_to_model(model: &mut AppModel, preview: &ResizePreview) {
    match preview {
        ResizePreview::WorkspaceColumnWidths {
            workspace_id,
            widths,
        } => {
            for (workspace_column_id, width) in widths {
                let _ =
                    model.set_workspace_column_width(*workspace_id, *workspace_column_id, *width);
            }
        }
        ResizePreview::WorkspaceWindowHeights {
            workspace_id,
            heights,
        } => {
            for (workspace_window_id, height) in heights {
                let _ =
                    model.set_workspace_window_height(*workspace_id, *workspace_window_id, *height);
            }
        }
        ResizePreview::WorkspaceWindowCorner {
            workspace_id,
            column_widths,
            window_heights,
        } => {
            for (workspace_column_id, width) in column_widths {
                let _ =
                    model.set_workspace_column_width(*workspace_id, *workspace_column_id, *width);
            }
            for (workspace_window_id, height) in window_heights {
                let _ =
                    model.set_workspace_window_height(*workspace_id, *workspace_window_id, *height);
            }
        }
        ResizePreview::WorkspaceWindowSplitRatio {
            workspace_id,
            workspace_window_id,
            path,
            ratio,
        } => {
            let _ = model.set_window_split_ratio(*workspace_id, *workspace_window_id, path, *ratio);
        }
        ResizePreview::PaneTabSplitRatio {
            workspace_id,
            pane_container_id,
            pane_tab_id,
            path,
            ratio,
        } => {
            let _ = model.set_pane_tab_split_ratio(
                *workspace_id,
                *pane_container_id,
                *pane_tab_id,
                path,
                *ratio,
            );
        }
    }
}

fn focused_split_ratio_targets<LeafId: Copy + Eq>(
    node: &taskers_domain::SplitLayoutNode<LeafId>,
    target: LeafId,
) -> Vec<(Vec<bool>, u16)> {
    let mut path = Vec::new();
    let mut targets = Vec::new();
    let _ = collect_focused_split_ratio_targets(node, target, &mut path, &mut targets);
    targets
}

fn collect_focused_split_ratio_targets<LeafId: Copy + Eq>(
    node: &taskers_domain::SplitLayoutNode<LeafId>,
    target: LeafId,
    path: &mut Vec<bool>,
    targets: &mut Vec<(Vec<bool>, u16)>,
) -> bool {
    match node {
        taskers_domain::SplitLayoutNode::Leaf { leaf_id } => *leaf_id == target,
        taskers_domain::SplitLayoutNode::Split { first, second, .. } => {
            path.push(false);
            if collect_focused_split_ratio_targets(first, target, path, targets) {
                path.pop();
                targets.push((path.clone(), EXPANDED_ACTIVE_SPLIT_RATIO));
                return true;
            }
            path.pop();

            path.push(true);
            if collect_focused_split_ratio_targets(second, target, path, targets) {
                path.pop();
                targets.push((path.clone(), COLLAPSED_INACTIVE_SPLIT_RATIO));
                return true;
            }
            path.pop();

            false
        }
    }
}

fn workspace_window_edge_handle_frame(frame: Frame, right_edge: bool, gap: i32) -> Frame {
    if right_edge {
        let center_x = frame.right() + gap / 2;
        Frame::new(
            center_x - RESIZE_HANDLE_THICKNESS_PX / 2,
            frame.y,
            RESIZE_HANDLE_THICKNESS_PX,
            frame.height.max(1),
        )
    } else {
        let center_y = frame.bottom() + gap / 2;
        Frame::new(
            frame.x,
            center_y - RESIZE_HANDLE_THICKNESS_PX / 2,
            frame.width.max(1),
            RESIZE_HANDLE_THICKNESS_PX,
        )
    }
}

fn workspace_window_outer_edge_handle_frame(frame: Frame, edge: WorkspaceOuterEdge) -> Frame {
    let x = match edge {
        WorkspaceOuterEdge::Left => frame.x - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX,
        WorkspaceOuterEdge::Right => frame.right(),
    };

    Frame::new(
        x,
        frame.y,
        WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX,
        frame.height.max(1),
    )
}

fn workspace_window_corner_handle_frame(frame: Frame, gap: i32) -> Frame {
    let center_x = frame.right() + gap / 2;
    let center_y = frame.bottom() + gap / 2;
    Frame::new(
        center_x - RESIZE_CORNER_SIZE_PX / 2,
        center_y - RESIZE_CORNER_SIZE_PX / 2,
        RESIZE_CORNER_SIZE_PX,
        RESIZE_CORNER_SIZE_PX,
    )
}

fn split_resize_handle_frame(frame: Frame, axis: SplitAxis, ratio: u16, gap: i32) -> Frame {
    let (first_frame, _) = split_frame(frame, axis, ratio, gap);
    match axis {
        SplitAxis::Horizontal => {
            let center_x = first_frame.right() + gap / 2;
            Frame::new(
                center_x - RESIZE_HANDLE_THICKNESS_PX / 2,
                frame.y,
                RESIZE_HANDLE_THICKNESS_PX,
                frame.height.max(1),
            )
        }
        SplitAxis::Vertical => {
            let center_y = first_frame.bottom() + gap / 2;
            Frame::new(
                frame.x,
                center_y - RESIZE_HANDLE_THICKNESS_PX / 2,
                frame.width.max(1),
                RESIZE_HANDLE_THICKNESS_PX,
            )
        }
    }
}

fn split_resize_cursor(axis: SplitAxis) -> ResizeHandleCursor {
    match axis {
        SplitAxis::Horizontal => ResizeHandleCursor::EastWest,
        SplitAxis::Vertical => ResizeHandleCursor::NorthSouth,
    }
}

fn resize_path_id(path: &[bool]) -> String {
    if path.is_empty() {
        return "root".into();
    }

    path.iter()
        .map(|segment| if *segment { '1' } else { '0' })
        .collect()
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
        self.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::PersistentDefault,
        });
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
    workspace_window_gap: i32,
) -> WorkspaceRenderContext {
    if !overview_mode {
        return WorkspaceRenderContext {
            overview_mode: false,
            overview_scale: 1.0,
            outer_padding_x: WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX,
            outer_padding_y: 0,
            viewport_width,
            viewport_height,
        };
    }

    let base_frames = workspace_window_placements(
        workspace,
        viewport_width,
        viewport_height,
        workspace_window_gap,
    )
    .into_iter()
    .map(|placement| placement.frame)
    .collect::<Vec<_>>();
    let base_metrics = canvas_metrics_from_frames(&base_frames, 0, 0);
    let outer_padding_x = metrics.workspace_padding;
    let outer_padding_y = metrics.workspace_padding;
    let available_width = (viewport_width - outer_padding_x * 2).max(1);
    let available_height = (viewport_height - outer_padding_y * 2).max(1);
    let overview_scale = (f64::from(available_width) / f64::from(base_metrics.width.max(1)))
        .min(f64::from(available_height) / f64::from(base_metrics.height.max(1)))
        .clamp(0.05, 1.0);

    WorkspaceRenderContext {
        overview_mode: true,
        overview_scale,
        outer_padding_x,
        outer_padding_y,
        viewport_width,
        viewport_height,
    }
}

fn workspace_display_window_placements(
    workspace: &Workspace,
    render_context: WorkspaceRenderContext,
    workspace_window_gap: i32,
) -> Vec<WorkspaceWindowPlacement> {
    let layout_viewport_width = if render_context.overview_mode {
        render_context.viewport_width
    } else {
        (render_context.viewport_width - render_context.outer_padding_x * 2).max(1)
    };
    let layout_viewport_height = if render_context.overview_mode {
        render_context.viewport_height
    } else {
        (render_context.viewport_height - render_context.outer_padding_y * 2).max(1)
    };
    workspace_window_placements(
        workspace,
        layout_viewport_width,
        layout_viewport_height,
        workspace_window_gap,
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
    _viewport_width: i32,
    _viewport_height: i32,
    workspace_window_gap: i32,
) -> Vec<WorkspaceWindowPlacement> {
    let ordered_columns = workspace.columns.values().collect::<Vec<_>>();
    if ordered_columns.is_empty() {
        return Vec::new();
    }

    let mut placements = Vec::new();
    let mut x = 0;
    for column in ordered_columns {
        let column_width = column.width.max(MIN_WORKSPACE_WINDOW_WIDTH);

        let mut y = 0;
        for window_id in &column.window_order {
            let Some(window) = workspace.windows.get(window_id) else {
                continue;
            };
            let window_height = window.height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
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
            y += window_height + workspace_window_gap;
        }

        x += column_width + workspace_window_gap;
    }

    placements
}

fn overview_scene_snapshot(
    columns: &[WorkspaceColumnSnapshot],
    prefer_live_preview: bool,
) -> OverviewSceneSnapshot {
    let mut cards = Vec::new();
    for (column_index, column) in columns.iter().enumerate() {
        let last_column_index = columns.len().saturating_sub(1);
        let last_row_index = column.windows.len().saturating_sub(1);
        for (row_index, window) in column.windows.iter().enumerate() {
            cards.push(OverviewWindowCardSnapshot {
                window_id: window.id,
                column_id: column.id,
                title: window.title.clone(),
                runtime: window.runtime.clone(),
                attention: window.attention,
                active: window.active,
                pane_count: window.pane_count,
                surface_count: window.surface_count,
                tab_count: window.tabs.len(),
                preview_mode: if prefer_live_preview {
                    OverviewPreviewModeSnapshot::LivePreferred
                } else {
                    OverviewPreviewModeSnapshot::Summary
                },
                preview_lines: overview_preview_lines(&window.layout),
                can_move_left: column_index > 0,
                can_move_right: column_index < last_column_index,
                can_move_up: row_index > 0,
                can_move_down: row_index < last_row_index,
            });
        }
    }

    OverviewSceneSnapshot {
        prefer_live_preview,
        cards,
    }
}

fn overview_preview_lines(layout: &LayoutNodeSnapshot) -> Vec<String> {
    let mut out = Vec::new();
    collect_overview_preview_lines(layout, &mut out);
    out.truncate(3);
    if out.is_empty() {
        out.push("No active pane content".to_string());
    }
    out
}

fn collect_overview_preview_lines(node: &LayoutNodeSnapshot, out: &mut Vec<String>) {
    match node {
        LayoutNodeSnapshot::Pane(pane) => {
            if let Some(surface) = pane
                .surfaces
                .iter()
                .find(|surface| surface.id == pane.active_surface)
                .or_else(|| pane.surfaces.first())
            {
                let mut line = surface.runtime.label.clone();
                if !surface.title.trim().is_empty() {
                    line.push_str(" · ");
                    line.push_str(surface.title.trim());
                }
                if let Some(status) = surface
                    .status_label
                    .as_deref()
                    .map(str::trim)
                    .filter(|status| !status.is_empty())
                {
                    line.push_str(" · ");
                    line.push_str(status);
                }
                out.push(line);
            } else {
                out.push(format!("{} pane", pane.runtime.label));
            }
        }
        LayoutNodeSnapshot::Split { first, second, .. } => {
            collect_overview_preview_lines(first, out);
            collect_overview_preview_lines(second, out);
        }
    }
}

fn workspace_canvas_metrics(
    placements: &[WorkspaceWindowPlacement],
    outer_padding_x: i32,
    outer_padding_y: i32,
) -> CanvasMetrics {
    let frames = placements
        .iter()
        .map(|placement| placement.frame)
        .collect::<Vec<_>>();
    canvas_metrics_from_frames(&frames, outer_padding_x, outer_padding_y)
}

fn clamped_workspace_viewport(
    workspace: &Workspace,
    viewport_width: i32,
    viewport_height: i32,
    workspace_window_gap: i32,
    viewport: taskers_domain::WorkspaceViewport,
) -> taskers_domain::WorkspaceViewport {
    let placements = workspace_window_placements(
        workspace,
        (viewport_width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2).max(1),
        viewport_height,
        workspace_window_gap,
    );
    let canvas = workspace_canvas_metrics(&placements, WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX, 0);
    let max_x = (canvas.width - viewport_width).max(0);
    let max_y = (canvas.height - viewport_height).max(0);

    taskers_domain::WorkspaceViewport {
        x: viewport.x.clamp(0, max_x),
        y: viewport.y.clamp(0, max_y),
    }
}

fn canvas_metrics_from_frames(
    frames: &[WindowFrame],
    outer_padding_x: i32,
    outer_padding_y: i32,
) -> CanvasMetrics {
    let min_x = frames.iter().map(|frame| frame.x).min().unwrap_or(0);
    let min_y = frames.iter().map(|frame| frame.y).min().unwrap_or(0);
    let offset_x = outer_padding_x - min_x;
    let offset_y = outer_padding_y - min_y;
    let width = frames
        .iter()
        .map(|frame| frame.right() + offset_x + outer_padding_x)
        .max()
        .unwrap_or(outer_padding_x.saturating_mul(2));
    let height = frames
        .iter()
        .map(|frame| frame.bottom() + offset_y + outer_padding_y)
        .max()
        .unwrap_or(outer_padding_y.saturating_mul(2));

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
        None,
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
    let ratio = i32::from(ratio.clamp(1, 999));
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

fn should_collapse_render_split(frame: Frame, axis: SplitAxis, gap: i32) -> bool {
    match axis {
        SplitAxis::Horizontal => {
            frame.width < (MIN_RENDERED_NATIVE_SURFACE_WIDTH_PX * 2 + gap.max(0))
        }
        SplitAxis::Vertical => {
            frame.height < (MIN_RENDERED_NATIVE_SURFACE_HEIGHT_PX * 2 + gap.max(0))
        }
    }
}

fn should_render_split_as_collapsed(frame: Frame, axis: SplitAxis, ratio: u16, gap: i32) -> bool {
    should_collapse_render_split(frame, axis, gap)
        || ratio <= COLLAPSED_INACTIVE_SPLIT_RATIO
        || ratio >= EXPANDED_ACTIVE_SPLIT_RATIO
}

fn layout_node_contains_pane(
    workspace: &Workspace,
    node: &taskers_domain::LayoutNode,
    pane_id: PaneId,
) -> bool {
    match node {
        taskers_domain::LayoutNode::Leaf { leaf_id } => workspace
            .pane_containers
            .get(leaf_id)
            .is_some_and(|container| container.contains_pane(pane_id)),
        taskers_domain::LayoutNode::Split { first, second, .. } => {
            layout_node_contains_pane(workspace, first, pane_id)
                || layout_node_contains_pane(workspace, second, pane_id)
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
    let terminal_gutter_x = match kind {
        PaneKind::Terminal => metrics.terminal_gutter_x,
        PaneKind::Browser => 0,
    };
    let tab_strip_height = if show_tab_strip {
        metrics.surface_tab_height
    } else {
        0
    };
    frame
        .inset(metrics.pane_border_width)
        .inset_horizontal(terminal_gutter_x)
        .inset_top(tab_strip_height + browser_toolbar_height)
}

fn pane_container_content_frame(frame: Frame, metrics: LayoutMetrics) -> Frame {
    frame
        .inset(metrics.pane_border_width)
        .inset_top(metrics.pane_header_height)
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
        .active_tab_record()
        .map(|tab| workspace_window_tab_attention(workspace, tab))
        .unwrap_or(AttentionState::Normal)
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
    window
        .active_tab_record()
        .map(|tab| workspace_window_tab_runtime_identity(workspace, tab, now))
        .unwrap_or_else(|| fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle))
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
    if let Some(agent_key) = surface_agent_key(surface) {
        return agent_key;
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
    if surface.interrupted_agent_resume.is_some() {
        return RuntimeStateSnapshot::Waiting;
    }
    surface_agent_state(surface, now)
        .map(runtime_state_from_agent_state)
        .unwrap_or(RuntimeStateSnapshot::Idle)
}

fn surface_agent_state(
    surface: &SurfaceRecord,
    now: OffsetDateTime,
) -> Option<taskers_domain::WorkspaceAgentState> {
    let session = surface.agent_session.as_ref()?;
    match session.state {
        taskers_domain::WorkspaceAgentState::Working
        | taskers_domain::WorkspaceAgentState::Waiting => Some(session.state),
        taskers_domain::WorkspaceAgentState::Completed
        | taskers_domain::WorkspaceAgentState::Failed => {
            (session.updated_at >= now - time::Duration::minutes(15)).then_some(session.state)
        }
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
    window
        .active_tab_record()
        .map(|tab| window_tab_primary_title(workspace, tab))
        .unwrap_or_else(|| "Workspace window".into())
}

fn workspace_window_tab_snapshot(
    workspace: &Workspace,
    tab: &WorkspaceWindowTabRecord,
    active_tab_id: WorkspaceWindowTabId,
    now: OffsetDateTime,
) -> WorkspaceWindowTabSnapshot {
    let pane_container_ids = tab.layout.leaves();
    let pane_count = pane_container_ids
        .iter()
        .filter_map(|pane_container_id| workspace.pane_containers.get(pane_container_id))
        .flat_map(|pane_container| pane_container.tabs.values())
        .map(|pane_tab| pane_tab.layout.leaves().len())
        .sum();
    let surface_count = pane_container_ids
        .iter()
        .filter_map(|pane_container_id| workspace.pane_containers.get(pane_container_id))
        .flat_map(|pane_container| pane_container.tabs.values())
        .flat_map(|pane_tab| pane_tab.layout.leaves())
        .filter_map(|pane_id| workspace.panes.get(&pane_id))
        .map(|pane| pane.surfaces.len())
        .sum();
    WorkspaceWindowTabSnapshot {
        id: tab.id,
        active: tab.id == active_tab_id,
        attention: workspace_window_tab_attention(workspace, tab),
        runtime: workspace_window_tab_runtime_identity(workspace, tab, now),
        title: window_tab_primary_title(workspace, tab),
        pane_count,
        surface_count,
    }
}

fn workspace_window_tab_attention(
    workspace: &Workspace,
    tab: &WorkspaceWindowTabRecord,
) -> AttentionState {
    tab.layout
        .leaves()
        .into_iter()
        .filter_map(|pane_container_id| workspace.pane_containers.get(&pane_container_id))
        .map(|pane_container| pane_container_attention(workspace, pane_container))
        .max_by_key(|attention| attention.rank())
        .unwrap_or(taskers_domain::AttentionState::Normal)
        .into()
}

fn workspace_window_tab_runtime_identity(
    workspace: &Workspace,
    tab: &WorkspaceWindowTabRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    dominant_runtime_identity(
        tab.layout
            .leaves()
            .into_iter()
            .filter_map(|pane_container_id| workspace.pane_containers.get(&pane_container_id))
            .map(|pane_container| {
                (
                    pane_container_runtime_identity(workspace, pane_container, now),
                    pane_container
                        .active_pane()
                        .is_some_and(|pane_id| pane_id == tab.active_pane),
                )
            }),
        fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle),
    )
}

fn window_tab_primary_title(workspace: &Workspace, tab: &WorkspaceWindowTabRecord) -> String {
    workspace
        .pane_containers
        .get(&tab.active_container)
        .and_then(|pane_container| pane_container.active_pane())
        .and_then(|pane_id| workspace.panes.get(&pane_id))
        .and_then(taskers_domain::PaneRecord::active_surface)
        .map(display_surface_title)
        .unwrap_or_else(|| "Workspace window".into())
}

fn display_surface_title(surface: &SurfaceRecord) -> String {
    match surface.kind {
        PaneKind::Terminal => display_terminal_title(surface),
        PaneKind::Browser => display_browser_title(&surface.metadata),
    }
}

fn surface_activity_label(surface: &SurfaceRecord, now: OffsetDateTime) -> Option<String> {
    if surface.interrupted_agent_resume.is_some() {
        return None;
    }
    let _ = active_agent_surface_state(surface, now)?;
    surface
        .agent_session
        .as_ref()
        .and_then(|session| session.latest_message.as_deref())
        .as_deref()
        .map(str::trim)
        .filter(|message| !message.is_empty())
        .map(str::to_owned)
}

fn surface_status_label(surface: &SurfaceRecord, now: OffsetDateTime) -> Option<String> {
    if surface.interrupted_agent_resume.is_some() {
        return Some("Interrupted".into());
    }
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

    match surface.attention {
        taskers_domain::AttentionState::WaitingInput => Some(AttentionRingState::Waiting),
        taskers_domain::AttentionState::Error => Some(AttentionRingState::Error),
        taskers_domain::AttentionState::Completed => Some(AttentionRingState::Completed),
        taskers_domain::AttentionState::Normal | taskers_domain::AttentionState::Busy => None,
    }
}

fn pane_notification_ring(pane: &taskers_domain::PaneRecord) -> Option<AttentionRingState> {
    dominant_attention_ring(pane.surfaces.values().filter_map(surface_notification_ring))
}

fn pane_tab_attention(
    workspace: &Workspace,
    pane_tab: &taskers_domain::PaneTabRecord,
) -> taskers_domain::AttentionState {
    pane_tab
        .layout
        .leaves()
        .into_iter()
        .filter_map(|pane_id| workspace.panes.get(&pane_id))
        .map(taskers_domain::PaneRecord::highest_attention)
        .max_by_key(|attention| attention.rank())
        .unwrap_or(taskers_domain::AttentionState::Normal)
}

fn pane_tab_runtime_identity(
    workspace: &Workspace,
    pane_tab: &taskers_domain::PaneTabRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    dominant_runtime_identity(
        pane_tab
            .layout
            .leaves()
            .into_iter()
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .map(|pane| {
                (
                    pane_runtime_identity(pane, now),
                    pane.id == pane_tab.active_pane,
                )
            }),
        fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle),
    )
}

fn pane_tab_primary_title(
    workspace: &Workspace,
    pane_tab: &taskers_domain::PaneTabRecord,
) -> String {
    workspace
        .panes
        .get(&pane_tab.active_pane)
        .and_then(taskers_domain::PaneRecord::active_surface)
        .map(display_surface_title)
        .unwrap_or_else(|| "Pane tab".into())
}

fn pane_container_attention(
    workspace: &Workspace,
    pane_container: &taskers_domain::PaneContainerRecord,
) -> taskers_domain::AttentionState {
    pane_container
        .tabs
        .values()
        .map(|pane_tab| pane_tab_attention(workspace, pane_tab))
        .max_by_key(|attention| attention.rank())
        .unwrap_or(taskers_domain::AttentionState::Normal)
}

fn pane_container_notification_ring(
    workspace: &Workspace,
    pane_container: &taskers_domain::PaneContainerRecord,
) -> Option<AttentionRingState> {
    dominant_attention_ring(
        pane_container
            .tabs
            .values()
            .flat_map(|pane_tab| pane_tab.layout.leaves())
            .filter_map(|pane_id| workspace.panes.get(&pane_id))
            .filter_map(pane_notification_ring),
    )
}

fn pane_container_runtime_identity(
    workspace: &Workspace,
    pane_container: &taskers_domain::PaneContainerRecord,
    now: OffsetDateTime,
) -> RuntimeIdentitySnapshot {
    pane_container
        .active_pane()
        .and_then(|pane_id| workspace.panes.get(&pane_id))
        .map(|pane| pane_runtime_identity(pane, now))
        .unwrap_or_else(|| fallback_runtime_identity("terminal", RuntimeStateSnapshot::Idle))
}

fn surface_agent_key(surface: &SurfaceRecord) -> Option<String> {
    surface
        .interrupted_agent_resume
        .as_ref()
        .map(|resume| resume.kind.clone())
        .or_else(|| {
            surface
                .agent_session
                .as_ref()
                .map(|session| session.kind.clone())
        })
        .or_else(|| {
            surface
                .agent_process
                .as_ref()
                .map(|process| process.kind.clone())
        })
}

fn display_terminal_title(surface: &SurfaceRecord) -> String {
    let context = terminal_context_label(&surface.metadata);

    if let Some(resume) = surface.interrupted_agent_resume.as_ref() {
        if let Some(context) = context.as_deref() {
            return format!("{} · {context}", resume.title);
        }
        return resume.title.clone();
    }

    if let Some(session) = surface.agent_session.as_ref() {
        if let Some(context) = context.as_deref() {
            return format!("{} · {context}", session.title);
        }
        return session.title.clone();
    }

    if let Some(process) = surface.agent_process.as_ref() {
        if let Some(context) = context.as_deref() {
            return format!("{} · {context}", process.title);
        }
        return process.title.clone();
    }

    if let Some(context) = context {
        return context;
    }

    if let Some(title) = surface
        .metadata
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

#[cfg(test)]
fn normalized_agent_key(value: Option<&str>) -> Option<String> {
    let normalized = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())?;
    match normalized.as_str() {
        "shell" => None,
        "claude code" | "claude-code" => Some("claude".into()),
        "codex" | "claude" | "opencode" | "aider" => Some(normalized),
        _ => None,
    }
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

    if matches!(
        basename.as_str(),
        "taskers-shell-wrapper.sh" | "taskers-agent-proxy.sh"
    ) {
        return true;
    }

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
    let _ = surface_count;
    true
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
        browser_profile_mode: surface.metadata.browser_profile_mode,
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
            profile_mode: descriptor.browser_profile_mode,
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

fn normalize_configured_shell(shell: Option<&str>) -> Option<String> {
    shell.and_then(|shell| {
        let trimmed = shell.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        COLLAPSED_INACTIVE_SPLIT_RATIO, EXPANDED_ACTIVE_SPLIT_RATIO, PixelSize,
        focused_split_ratio_targets, pane_container_content_frame,
    };
    use taskers_control::ControlCommand;
    use taskers_core::AppState;
    use taskers_domain::{
        AppModel, AttentionState as DomainAttentionState, InterruptedAgentResume, NotificationId,
        NotificationItem, PaneId, SignalKind,
    };
    use taskers_ghostty::BackendChoice;
    use taskers_runtime::ShellLaunchSpec;
    use time::OffsetDateTime;

    use super::{
        BootstrapModel, BrowserMountSpec, BrowserProfileMode, DEFAULT_BROWSER_HOME,
        DEFAULT_WORKSPACE_WINDOW_GAP, Direction, EmbeddedTerminalSettingsSnapshot, HostCommand,
        HostEvent, LayoutMetrics, MAX_WORKSPACE_WINDOW_GAP, MIN_RENDERED_NATIVE_SURFACE_WIDTH_PX,
        MIN_WORKSPACE_WINDOW_GAP, MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_WIDTH,
        NotificationPreferencesSnapshot, ResizeHandleTarget, ResizePreview, RuntimeCapability,
        RuntimeStatus, SharedCore, ShellAction, ShellDragMode, ShellSection, ShortcutAction,
        ShortcutPreset, SurfaceDragSessionSnapshot, SurfaceMountSpec,
        WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX, WorkspaceDirection, WorkspaceOuterEdge,
        WorkspaceWindowMoveTarget, WorkspaceWindowSnapshot, default_preview_app_state,
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
                terminal_persistence: RuntimeCapability::Ready,
            },
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: super::ShortcutPreset::Balanced,
            configured_shell: None,
            embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot::default(),
            notification_preferences: NotificationPreferencesSnapshot::default(),
            render_live_surfaces_in_overview: true,
            workspace_window_gap: DEFAULT_WORKSPACE_WINDOW_GAP,
        }
    }

    fn terminal_only_bootstrap() -> BootstrapModel {
        BootstrapModel {
            app_state: AppState::new(
                AppModel::new("Main"),
                default_session_path_for_preview("taskers-preview-terminal-only"),
                BackendChoice::Mock,
                ShellLaunchSpec::fallback(),
                None,
            )
            .expect("terminal-only app state"),
            runtime_status: RuntimeStatus {
                ghostty_runtime: RuntimeCapability::Ready,
                shell_integration: RuntimeCapability::Ready,
                terminal_host: RuntimeCapability::Fallback {
                    message: "Probe failed".into(),
                },
                terminal_persistence: RuntimeCapability::Ready,
            },
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: super::ShortcutPreset::Balanced,
            configured_shell: None,
            embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot::default(),
            notification_preferences: NotificationPreferencesSnapshot::default(),
            render_live_surfaces_in_overview: true,
            workspace_window_gap: DEFAULT_WORKSPACE_WINDOW_GAP,
        }
    }

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new(prefix: &str) -> Self {
            let unique = format!(
                "{prefix}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn run_command(command: &mut Command) {
        let output = command.output().expect("run command");
        assert!(
            output.status.success(),
            "command failed: status={:?}\nstdout={}\nstderr={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    fn command_available(program: &str) -> bool {
        Command::new(program)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn init_git_vcs_fixture() -> TestDir {
        let fixture = TestDir::new("taskers-shell-core-vcs");
        let remote_path = fixture.path().join("remote.git");
        let repo_path = fixture.path().join("repo");
        fs::create_dir_all(&repo_path).expect("create repo dir");

        run_command(
            Command::new("git")
                .arg("init")
                .arg("--bare")
                .arg(&remote_path),
        );
        run_command(Command::new("git").arg("init").arg(&repo_path));
        run_command(Command::new("git").arg("-C").arg(&repo_path).args([
            "symbolic-ref",
            "HEAD",
            "refs/heads/main",
        ]));
        run_command(Command::new("git").arg("-C").arg(&repo_path).args([
            "config",
            "user.name",
            "Taskers Tests",
        ]));
        run_command(Command::new("git").arg("-C").arg(&repo_path).args([
            "config",
            "user.email",
            "tests@example.com",
        ]));

        fs::write(repo_path.join("committed.txt"), "base\n").expect("write committed seed");
        fs::write(repo_path.join("working.txt"), "base\n").expect("write working seed");
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["add", "."]),
        );
        run_command(Command::new("git").arg("-C").arg(&repo_path).args([
            "commit",
            "-m",
            "chore: base fixture",
        ]));
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["remote", "add", "origin"])
                .arg(&remote_path),
        );
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["push", "-u", "origin", "main"]),
        );

        fs::write(repo_path.join("committed.txt"), "base\nunpushed\n")
            .expect("write unpushed change");
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["add", "committed.txt"]),
        );
        run_command(Command::new("git").arg("-C").arg(&repo_path).args([
            "commit",
            "-m",
            "feat: add unpushed change",
        ]));

        fs::write(repo_path.join("working.txt"), "base\nworking tree change\n")
            .expect("write working tree change");

        fixture
    }

    fn init_jj_vcs_fixture() -> Option<TestDir> {
        if !command_available("jj") {
            return None;
        }

        let fixture = TestDir::new("taskers-shell-core-jj");
        let remote_path = fixture.path().join("remote.git");
        let repo_path = fixture.path().join("repo");

        run_command(
            Command::new("git")
                .arg("init")
                .arg("--bare")
                .arg(&remote_path),
        );
        run_command(
            Command::new("jj")
                .args(["git", "init", "--colocate"])
                .arg(&repo_path),
        );
        run_command(
            Command::new("git")
                .arg("-C")
                .arg(&repo_path)
                .args(["remote", "add", "origin"])
                .arg(&remote_path),
        );

        fs::write(repo_path.join("committed.txt"), "base\n").expect("write committed seed");
        fs::write(repo_path.join("working.txt"), "base\n").expect("write working seed");
        run_command(Command::new("jj").arg("-R").arg(&repo_path).args([
            "describe",
            "-m",
            "chore: base fixture",
        ]));
        run_command(
            Command::new("jj")
                .arg("-R")
                .arg(&repo_path)
                .args(["bookmark", "create", "main"]),
        );
        run_command(Command::new("jj").arg("-R").arg(&repo_path).args([
            "git",
            "push",
            "--bookmark",
            "main",
        ]));
        run_command(Command::new("jj").arg("-R").arg(&repo_path).args([
            "describe",
            "-m",
            "feat: add unpushed change",
        ]));

        fs::write(repo_path.join("committed.txt"), "base\nunpushed\n")
            .expect("write unpushed change");
        fs::write(repo_path.join("working.txt"), "base\nworking tree change\n")
            .expect("write working tree change");

        Some(fixture)
    }

    fn model_with_terminal_cwd(cwd: &Path) -> AppModel {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface");
        let surface = model
            .workspaces
            .get_mut(&workspace_id)
            .and_then(|workspace| workspace.panes.get_mut(&pane_id))
            .and_then(|pane| pane.surfaces.get_mut(&surface_id))
            .expect("surface record");
        surface.metadata.cwd = Some(cwd.display().to_string());
        model
    }

    fn preview_app_state_with_model(model: AppModel, label: &str) -> AppState {
        AppState::new(
            model,
            default_session_path_for_preview(label),
            BackendChoice::Mock,
            ShellLaunchSpec::fallback(),
            None,
        )
        .expect("preview app state")
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
                None,
            )
            .expect("preview app state"),
            ..bootstrap()
        }
    }

    fn window_snapshot(
        snapshot: &super::ShellSnapshot,
        window_id: taskers_domain::WorkspaceWindowId,
    ) -> &WorkspaceWindowSnapshot {
        snapshot
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == window_id)
            .expect("window snapshot")
    }

    fn bootstrap_with_model(model: AppModel, name: &str) -> BootstrapModel {
        BootstrapModel {
            app_state: super::AppState::new(
                model,
                default_session_path_for_preview(name),
                super::BackendChoice::Mock,
                super::ShellLaunchSpec::fallback(),
                None,
            )
            .expect("preview app state"),
            ..bootstrap()
        }
    }

    fn find_pane<'a>(
        node: &'a super::LayoutNodeSnapshot,
        pane_id: taskers_domain::PaneId,
    ) -> Option<&'a super::LivePaneSnapshot> {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => {
                find_live_pane_in_layout(&pane.layout, pane_id)
            }
            super::LayoutNodeSnapshot::Split { first, second, .. } => {
                find_pane(first, pane_id).or_else(|| find_pane(second, pane_id))
            }
        }
    }

    fn find_pane_frame(
        node: &super::LayoutNodeSnapshot,
        pane_id: taskers_domain::PaneId,
        frame: super::Frame,
        metrics: super::LayoutMetrics,
    ) -> Option<super::Frame> {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => find_live_pane_frame(
                &pane.layout,
                pane_id,
                super::pane_container_content_frame(frame, metrics),
                metrics.split_gap,
            ),
            super::LayoutNodeSnapshot::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = (ratio.clamp(0.001, 0.999) * 1000.0).round() as u16;
                let (first_frame, second_frame) =
                    split_frame(frame, *axis, ratio, metrics.split_gap);
                find_pane_frame(first, pane_id, first_frame, metrics)
                    .or_else(|| find_pane_frame(second, pane_id, second_frame, metrics))
            }
        }
    }

    fn collect_pane_ids(
        node: &super::LayoutNodeSnapshot,
        pane_ids: &mut Vec<taskers_domain::PaneId>,
    ) {
        match node {
            super::LayoutNodeSnapshot::Pane(pane) => {
                collect_live_pane_ids(&pane.layout, pane_ids);
            }
            super::LayoutNodeSnapshot::Split { first, second, .. } => {
                collect_pane_ids(first, pane_ids);
                collect_pane_ids(second, pane_ids);
            }
        }
    }

    fn find_live_pane_in_layout<'a>(
        node: &'a super::PaneTabLayoutSnapshot,
        pane_id: taskers_domain::PaneId,
    ) -> Option<&'a super::LivePaneSnapshot> {
        match node {
            super::PaneTabLayoutSnapshot::Pane(pane) => (pane.id == pane_id).then_some(pane),
            super::PaneTabLayoutSnapshot::Split { first, second, .. } => {
                find_live_pane_in_layout(first, pane_id)
                    .or_else(|| find_live_pane_in_layout(second, pane_id))
            }
        }
    }

    fn find_live_pane_frame(
        node: &super::PaneTabLayoutSnapshot,
        pane_id: taskers_domain::PaneId,
        frame: super::Frame,
        gap: i32,
    ) -> Option<super::Frame> {
        match node {
            super::PaneTabLayoutSnapshot::Pane(pane) => (pane.id == pane_id).then_some(frame),
            super::PaneTabLayoutSnapshot::Split {
                axis,
                ratio,
                first,
                second,
            } => {
                let ratio = (ratio.clamp(0.001, 0.999) * 1000.0).round() as u16;
                let (first_frame, second_frame) = split_frame(frame, *axis, ratio, gap);
                find_live_pane_frame(first, pane_id, first_frame, gap)
                    .or_else(|| find_live_pane_frame(second, pane_id, second_frame, gap))
            }
        }
    }

    fn collect_live_pane_ids(
        node: &super::PaneTabLayoutSnapshot,
        pane_ids: &mut Vec<taskers_domain::PaneId>,
    ) {
        match node {
            super::PaneTabLayoutSnapshot::Pane(pane) => pane_ids.push(pane.id),
            super::PaneTabLayoutSnapshot::Split { first, second, .. } => {
                collect_live_pane_ids(first, pane_ids);
                collect_live_pane_ids(second, pane_ids);
            }
        }
    }

    fn surface_with_metadata(
        kind: taskers_domain::PaneKind,
        metadata: taskers_domain::PaneMetadata,
    ) -> taskers_domain::SurfaceRecord {
        let agent_kind = metadata
            .agent_kind
            .as_deref()
            .and_then(|kind| super::normalized_agent_key(Some(kind)))
            .or_else(|| {
                metadata
                    .agent_title
                    .as_deref()
                    .and_then(|title| super::normalized_agent_key(Some(title)))
            });
        let process = agent_kind
            .clone()
            .map(|kind| taskers_domain::SurfaceAgentProcess {
                id: taskers_domain::SessionId::new(),
                kind: kind.clone(),
                title: metadata
                    .agent_title
                    .clone()
                    .unwrap_or_else(|| super::runtime_label(&kind)),
                started_at: metadata
                    .last_signal_at
                    .unwrap_or_else(OffsetDateTime::now_utc),
            });
        let session = agent_kind.clone().and_then(|kind| {
            metadata
                .agent_state
                .map(|state| taskers_domain::SurfaceAgentSession {
                    id: taskers_domain::SessionId::new(),
                    kind: kind.clone(),
                    title: metadata
                        .agent_title
                        .clone()
                        .unwrap_or_else(|| super::runtime_label(&kind)),
                    state,
                    latest_message: metadata.latest_agent_message.clone(),
                    updated_at: metadata
                        .last_signal_at
                        .unwrap_or_else(OffsetDateTime::now_utc),
                })
        });
        let mut surface = taskers_domain::SurfaceRecord::new(kind);
        surface.metadata = metadata;
        surface.agent_process = process;
        surface.agent_session = session;
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
    fn terminal_surface_titles_treat_taskers_shell_wrapper_as_generic() {
        let surface = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                title: Some("/run/user/1000/taskers/shell/taskers-shell-wrapper.sh".into()),
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
    fn notification_rings_cover_agent_notifications_without_agent_kind_metadata() {
        let mut waiting = surface_with_metadata(
            taskers_domain::PaneKind::Terminal,
            taskers_domain::PaneMetadata {
                agent_title: Some("Codex".into()),
                agent_state: Some(taskers_domain::WorkspaceAgentState::Waiting),
                latest_agent_message: Some("Need input".into()),
                ..taskers_domain::PaneMetadata::default()
            },
        );
        waiting.attention = taskers_domain::AttentionState::WaitingInput;

        assert_eq!(
            super::surface_notification_ring(&waiting),
            Some(super::AttentionRingState::Waiting)
        );
        assert_eq!(super::runtime_key(&waiting), "codex");
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
            let pane_container = workspace
                .pane_containers
                .values()
                .find(|pane_container| pane_container.contains_pane(first_pane_id))
                .expect("pane container");
            let pane_tab = pane_container
                .tabs
                .values()
                .find(|pane_tab| pane_tab.layout.contains(first_pane_id))
                .expect("pane tab");
            pane_tab
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
            first_surface.agent_session = Some(taskers_domain::SurfaceAgentSession {
                id: taskers_domain::SessionId::new(),
                kind: "codex".into(),
                title: "Codex".into(),
                state: taskers_domain::WorkspaceAgentState::Working,
                latest_message: None,
                updated_at: now,
            });
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
            second_surface.agent_session = Some(taskers_domain::SurfaceAgentSession {
                id: taskers_domain::SessionId::new(),
                kind: "claude".into(),
                title: "Claude".into(),
                state: taskers_domain::WorkspaceAgentState::Failed,
                latest_message: None,
                updated_at: now,
            });
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
    fn portal_plan_carries_pane_notification_ring_for_active_surface_host() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let agent_surface_id = model
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("agent surface");
        let browser_surface_id = model
            .create_surface(workspace_id, pane_id, taskers_domain::PaneKind::Browser)
            .expect("create browser");

        {
            let workspace = model.workspaces.get_mut(&workspace_id).expect("workspace");
            let browser_surface = workspace
                .panes
                .get_mut(&pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&browser_surface_id))
                .expect("browser surface record");
            browser_surface.attention = taskers_domain::AttentionState::Normal;

            let agent_surface = workspace
                .panes
                .get_mut(&pane_id)
                .and_then(|pane| pane.surfaces.get_mut(&agent_surface_id))
                .expect("agent surface record");
            agent_surface.metadata.agent_kind = Some("codex".into());
            agent_surface.attention = taskers_domain::AttentionState::WaitingInput;
        }

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-portal-notification-ring",
        ));
        let portal_plan = core
            .snapshot()
            .portal
            .panes
            .into_iter()
            .find(|plan| plan.pane_id == pane_id)
            .expect("portal plan");

        assert_eq!(portal_plan.surface_id, browser_surface_id);
        assert_eq!(
            portal_plan.notification_ring,
            Some(super::AttentionRingState::Waiting)
        );
    }

    #[test]
    fn portal_plan_carries_ring_for_agent_notifications_without_prior_agent_kind() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface");

        model
            .create_agent_notification(
                taskers_domain::AgentTarget::Surface {
                    workspace_id,
                    pane_id,
                    surface_id,
                },
                taskers_domain::SignalKind::Notification,
                Some("Codex".into()),
                None,
                None,
                "Need input".into(),
                taskers_domain::AttentionState::WaitingInput,
            )
            .expect("notification");

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-agent-notification-ring",
        ));
        let portal_plan = core
            .snapshot()
            .portal
            .panes
            .into_iter()
            .find(|plan| plan.pane_id == pane_id)
            .expect("portal plan");

        assert_eq!(
            portal_plan.notification_ring,
            Some(super::AttentionRingState::Waiting)
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
    fn workspace_window_snapshots_expose_window_tabs() {
        let core = SharedCore::bootstrap(bootstrap());
        let window_id = core.snapshot().current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindowTab { window_id });

        let snapshot = core.snapshot();
        let active_window = snapshot
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == window_id)
            .expect("active window");

        assert_eq!(active_window.tabs.len(), 2);
        assert_eq!(active_window.active_tab, active_window.tabs[1].id);
        assert!(active_window.tabs[1].active);
        assert_eq!(
            active_window.active_pane,
            snapshot.current_workspace.active_pane
        );
    }

    #[test]
    fn canceling_resize_preview_restores_column_width_snapshot() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let mut widths = snapshot
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, column.width))
            .collect::<Vec<_>>();
        let original_width = widths[0].1;
        widths[0].1 += 180;
        widths[1].1 -= 180;

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths,
            },
        });

        assert_eq!(
            core.snapshot().current_workspace.columns[0].width,
            original_width + 180
        );

        core.dispatch_shell_action(ShellAction::CancelResizePreview);

        assert_eq!(
            core.snapshot().current_workspace.columns[0].width,
            original_width
        );
    }

    #[test]
    fn committing_corner_resize_updates_model_state() {
        let core = SharedCore::bootstrap(bootstrap());
        let initial_snapshot = core.snapshot();
        let top_left_window_id = initial_snapshot.current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::FocusWorkspaceWindow {
            window_id: top_left_window_id,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let top_left_window = window_snapshot(&snapshot, top_left_window_id);
        let top_left_column_id = top_left_window.column_id;
        let model_before = core.inner.lock().app_state.snapshot_model();
        let workspace_before = model_before
            .workspaces
            .get(&workspace_id)
            .expect("workspace");
        let column_before = workspace_before
            .columns
            .get(&top_left_column_id)
            .expect("column")
            .width;
        let mut column_widths = snapshot
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, column.width))
            .collect::<Vec<_>>();
        let left_column_index = column_widths
            .iter()
            .position(|(column_id, _)| *column_id == top_left_column_id)
            .expect("left column index");
        column_widths[left_column_index].1 += 120;
        column_widths[left_column_index + 1].1 -= 120;
        let mut window_heights = snapshot
            .current_workspace
            .columns
            .iter()
            .find(|column| column.id == top_left_column_id)
            .expect("left column")
            .windows
            .iter()
            .map(|window| (window.id, window.frame.height))
            .collect::<Vec<_>>();
        let upper_window_index = window_heights
            .iter()
            .position(|(window_id, _)| *window_id == top_left_window_id)
            .expect("upper window index");
        window_heights[upper_window_index].1 += 80;
        window_heights[upper_window_index + 1].1 -= 80;

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceWindowCorner {
                workspace_id,
                column_widths,
                window_heights,
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        let model_after = core.inner.lock().app_state.snapshot_model();
        let workspace_after = model_after
            .workspaces
            .get(&workspace_id)
            .expect("workspace");
        assert_eq!(
            workspace_after
                .columns
                .get(&top_left_column_id)
                .expect("column")
                .width,
            column_before + 120
        );
        assert_eq!(
            workspace_after
                .windows
                .get(&top_left_window_id)
                .expect("window")
                .height,
            snapshot
                .current_workspace
                .columns
                .iter()
                .find(|column| column.id == top_left_column_id)
                .expect("left column")
                .windows[0]
                .frame
                .height
                + 80
        );
    }

    #[test]
    fn pane_split_resize_preview_updates_handle_ratio_and_commits() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::SplitTerminal { pane_id: None });

        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let active_pane_id = snapshot.current_workspace.active_pane;
        let model = core.inner.lock().app_state.snapshot_model();
        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let window_id = workspace.window_for_pane(active_pane_id).expect("window");
        let window = workspace.windows.get(&window_id).expect("window");
        let pane_container_id = window.active_container().expect("active container");
        let pane_tab_id = workspace
            .pane_containers
            .get(&pane_container_id)
            .and_then(|pane_container| pane_container.tab_for_pane(active_pane_id))
            .expect("pane tab");

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::PaneTabSplitRatio {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path: Vec::new(),
                ratio: 700,
            },
        });

        let preview_snapshot = core.snapshot();
        let handle = preview_snapshot
            .resize_handles
            .iter()
            .find(|handle| {
                matches!(
                    &handle.target,
                    ResizeHandleTarget::PaneTabSplit {
                        pane_container_id: target_container_id,
                        pane_tab_id: target_tab_id,
                        path,
                        initial_ratio,
                        ..
                    } if *target_container_id == pane_container_id
                        && *target_tab_id == pane_tab_id
                        && path.is_empty()
                        && *initial_ratio == 700
                )
            })
            .expect("pane split resize handle");
        assert!(!handle.id.is_empty());

        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        let committed_model = core.inner.lock().app_state.snapshot_model();
        let committed_workspace = committed_model
            .workspaces
            .get(&workspace_id)
            .expect("workspace");
        let pane_tab = committed_workspace
            .pane_containers
            .get(&pane_container_id)
            .and_then(|pane_container| pane_container.tabs.get(&pane_tab_id))
            .expect("pane tab");
        let taskers_domain::PaneTabLayoutNode::Split { ratio, .. } = &pane_tab.layout else {
            panic!("expected split layout");
        };
        assert_eq!(*ratio, 700);
    }

    #[test]
    fn resize_handles_are_hidden_in_overview_mode() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        assert!(!core.snapshot().resize_handles.is_empty());

        core.dispatch_shell_action(ShellAction::ToggleOverview);

        assert!(core.snapshot().resize_handles.is_empty());
    }

    #[test]
    fn single_workspace_window_exposes_outer_edge_resize_handles() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let active_window = snapshot
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .find(|window| window.id == snapshot.current_workspace.active_window_id)
            .expect("active window");
        let left_handle = snapshot
            .resize_handles
            .iter()
            .find(|handle| {
                matches!(
                    &handle.target,
                    ResizeHandleTarget::WorkspaceColumnOuterEdge {
                        column_index: 0,
                        edge: WorkspaceOuterEdge::Left,
                        ..
                    }
                )
            })
            .expect("left outer edge handle");
        let right_handle = snapshot
            .resize_handles
            .iter()
            .find(|handle| {
                matches!(
                    &handle.target,
                    ResizeHandleTarget::WorkspaceColumnOuterEdge {
                        column_index: 0,
                        edge: WorkspaceOuterEdge::Right,
                        ..
                    }
                )
            })
            .expect("right outer edge handle");

        assert!(left_handle.frame.right() <= active_window.frame.x);
        assert!(right_handle.frame.x >= active_window.frame.right());
    }

    #[test]
    fn wide_three_column_workspace_can_shrink_left_column_below_old_limit() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(2048, 900));
        let left_window_id = core.snapshot().current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let before = core.snapshot();
        let workspace_id = before.current_workspace.id;
        let left_window = window_snapshot(&before, left_window_id);
        let before_width = left_window.frame.width;
        let mut column_widths = before
            .current_workspace
            .columns
            .iter()
            .map(|column| {
                (
                    column.id,
                    column.windows.first().expect("column window").frame.width,
                )
            })
            .collect::<Vec<_>>();
        column_widths[1].1 += column_widths[0].1 - taskers_domain::MIN_WORKSPACE_WINDOW_WIDTH;
        column_widths[0].1 = taskers_domain::MIN_WORKSPACE_WINDOW_WIDTH;

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: column_widths,
            },
        });

        let after = core.snapshot();
        let after_width = window_snapshot(&after, left_window_id).frame.width;

        assert!(
            after_width < before_width,
            "expected left column to keep shrinking in a three-column layout"
        );
        assert!(
            after_width < 720,
            "expected left column to shrink below the old hard limit"
        );
    }

    #[test]
    fn creating_horizontal_workspace_window_preserves_existing_width_and_applies_new_window_default_width()
     {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let before = core.snapshot();
        let first_window_id = before.current_workspace.active_window_id;
        let first_window_before = window_snapshot(&before, first_window_id);
        let expected_width = taskers_domain::DEFAULT_WORKSPACE_WINDOW_WIDTH;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let snapshot = core.snapshot();
        let first_window = window_snapshot(&snapshot, first_window_id);
        let active_window = window_snapshot(&snapshot, snapshot.current_workspace.active_window_id);

        assert_eq!(active_window.frame.width, expected_width);
        assert_eq!(first_window.frame.width, first_window_before.frame.width);
        assert!(
            snapshot.current_workspace.canvas_width
                >= first_window.frame.width
                    + active_window.frame.width
                    + DEFAULT_WORKSPACE_WINDOW_GAP
        );
    }

    #[test]
    fn creating_vertical_workspace_window_preserves_existing_height_and_applies_new_window_default_height()
     {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let before = core.snapshot();
        let first_window_before =
            window_snapshot(&before, before.current_workspace.active_window_id);
        let expected_height = taskers_domain::DEFAULT_WORKSPACE_WINDOW_HEIGHT;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let snapshot = core.snapshot();
        let first_window = window_snapshot(&snapshot, first_window_before.id);
        let active_window = window_snapshot(&snapshot, snapshot.current_workspace.active_window_id);

        assert!(
            (active_window.frame.height - expected_height).abs() <= 4,
            "expected active window height to stay near half the visible viewport (expected {expected_height}, got {})",
            active_window.frame.height
        );
        assert_eq!(first_window.frame.height, first_window_before.frame.height);
    }

    #[test]
    fn extracting_window_tab_uses_persistent_default_width() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(700, 900));
        let source_window_id = core.snapshot().current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindowTab {
            window_id: source_window_id,
        });
        let before = core.snapshot();
        let active_window = window_snapshot(&before, source_window_id);
        let tab_id = active_window.tabs[1].id;
        let expected_width = taskers_domain::DEFAULT_WORKSPACE_WINDOW_WIDTH;

        core.dispatch_shell_action(ShellAction::ExtractWorkspaceWindowTab {
            source_window_id,
            tab_id,
            target: WorkspaceWindowMoveTarget::ColumnAfter {
                column_id: active_window.column_id,
            },
        });

        let snapshot = core.snapshot();
        let active_window = window_snapshot(&snapshot, snapshot.current_workspace.active_window_id);
        assert_eq!(active_window.frame.width, expected_width);
    }

    #[test]
    fn moving_surface_to_workspace_uses_persistent_default_width() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(700, 900));
        let source_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(source_pane_id),
            profile_mode: BrowserProfileMode::PersistentDefault,
        });
        let before = core.snapshot();
        let moved_surface_id = find_pane(&before.current_workspace.layout, source_pane_id)
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
        let expected_width = taskers_domain::DEFAULT_WORKSPACE_WINDOW_WIDTH;

        core.dispatch_shell_action(ShellAction::MoveSurfaceToWorkspace {
            source_pane_id,
            surface_id: moved_surface_id,
            target_workspace_id,
        });

        let snapshot = core.snapshot();
        let active_window = window_snapshot(&snapshot, snapshot.current_workspace.active_window_id);
        assert_eq!(active_window.frame.width, expected_width);
    }

    #[test]
    fn wide_three_column_workspace_keeps_total_width_beyond_viewport() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let workspace_id = core.snapshot().current_workspace.id;
        let widths = core
            .snapshot()
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, 720))
            .collect::<Vec<_>>();

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths,
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        let snapshot = core.snapshot();
        let total_column_width = snapshot
            .current_workspace
            .columns
            .iter()
            .map(|column| column.windows.first().expect("column window").frame.width)
            .sum::<i32>();
        let total_gap_width = DEFAULT_WORKSPACE_WINDOW_GAP
            * snapshot.current_workspace.columns.len().saturating_sub(1) as i32;

        assert_eq!(total_column_width, 2160);
        assert!(
            snapshot.current_workspace.canvas_width >= total_column_width + total_gap_width,
            "expected canvas width to grow beyond the viewport for wide workspaces"
        );
    }

    #[test]
    fn resizing_outer_window_preserves_persisted_top_level_extents() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let column_widths = snapshot
            .current_workspace
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.id, if index == 0 { 820 } else { 640 }))
            .collect::<Vec<_>>();
        let right_column = snapshot
            .current_workspace
            .columns
            .iter()
            .find(|column| column.windows.len() > 1)
            .expect("stacked column");
        let mut window_heights = snapshot
            .current_workspace
            .columns
            .iter()
            .filter(|column| column.windows.len() == 1)
            .flat_map(|column| {
                column
                    .windows
                    .iter()
                    .map(|window| (window.id, window.frame.height))
            })
            .collect::<Vec<_>>();
        window_heights.extend(
            right_column
                .windows
                .iter()
                .enumerate()
                .map(|(index, window)| (window.id, if index == 0 { 520 } else { 680 }))
                .collect::<Vec<_>>(),
        );

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: column_widths.clone(),
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);
        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceWindowHeights {
                workspace_id,
                heights: window_heights.clone(),
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        core.set_window_size(PixelSize::new(1600, 980));

        let after = core.snapshot();
        let actual_widths = after
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, column.width))
            .collect::<Vec<_>>();
        let actual_heights = after
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .map(|window| (window.id, window.frame.height))
            .collect::<Vec<_>>();

        assert_eq!(actual_widths, column_widths);
        assert_eq!(actual_heights, window_heights);
    }

    #[test]
    fn focusing_tall_stacked_window_scrolls_vertically_without_resizing_windows() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let before = core.snapshot();
        let workspace_id = before.current_workspace.id;
        let heights = before
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .enumerate()
            .map(|(index, window)| (window.id, if index == 0 { 520 } else { 560 }))
            .collect::<Vec<_>>();
        let target_window_id = before
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .last()
            .map(|window| window.id)
            .expect("bottom window");

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceWindowHeights {
                workspace_id,
                heights: heights.clone(),
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        core.dispatch_shell_action(ShellAction::FocusWorkspaceWindow {
            window_id: target_window_id,
        });

        let after = core.snapshot();
        let actual_heights = after
            .current_workspace
            .columns
            .iter()
            .flat_map(|column| column.windows.iter())
            .map(|window| (window.id, window.frame.height))
            .collect::<Vec<_>>();

        assert!(after.current_workspace.viewport_y > 0);
        assert_eq!(actual_heights, heights);
    }

    #[test]
    fn toggling_overview_does_not_mutate_persisted_top_level_extents() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let before = core.snapshot();
        let workspace_id = before.current_workspace.id;
        let widths = before
            .current_workspace
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.id, if index == 0 { 780 } else { 620 }))
            .collect::<Vec<_>>();

        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths: widths.clone(),
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        let persisted_before = {
            let guard = core.inner.lock();
            let model = guard.app_state.snapshot_model();
            let workspace = model.workspaces.get(&workspace_id).expect("workspace");
            workspace
                .columns
                .values()
                .map(|column| (column.id, column.width))
                .collect::<Vec<_>>()
        };

        core.dispatch_shell_action(ShellAction::ToggleOverview);
        core.dispatch_shell_action(ShellAction::SetOverviewLiveSurfaces { enabled: false });
        core.dispatch_shell_action(ShellAction::ToggleOverview);

        let persisted_after = {
            let guard = core.inner.lock();
            let model = guard.app_state.snapshot_model();
            let workspace = model.workspaces.get(&workspace_id).expect("workspace");
            workspace
                .columns
                .values()
                .map(|column| (column.id, column.width))
                .collect::<Vec<_>>()
        };

        assert_eq!(persisted_before, widths);
        assert_eq!(persisted_after, widths);
    }

    #[test]
    fn resizing_active_window_right_preserves_requested_growth_in_wide_workspace() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let left_window_id = core.snapshot().current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::FocusWorkspaceWindow {
            window_id: left_window_id,
        });

        let before = window_snapshot(&core.snapshot(), left_window_id)
            .frame
            .width;
        let workspace_id = core.snapshot().current_workspace.id;
        {
            let mut inner = core.inner.lock();
            assert!(inner.dispatch_control(ControlCommand::ResizeActiveWindow {
                workspace_id,
                direction: Direction::Right,
                amount: 180,
            }));
        }

        let after = window_snapshot(&core.snapshot(), left_window_id)
            .frame
            .width;
        assert_eq!(
            after,
            before + 180,
            "expected active window growth to push the workspace wider instead of being refit"
        );
    }

    #[test]
    fn terminal_portal_frames_use_tight_terminal_gutter_by_default() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let metrics = LayoutMetrics::default();
        let terminal_plan = snapshot
            .portal
            .panes
            .iter()
            .find(|plan| matches!(plan.mount, SurfaceMountSpec::Terminal(_)))
            .expect("terminal plan");

        assert_eq!(metrics.terminal_gutter_x, 0);
        assert_eq!(
            terminal_plan.frame.x,
            terminal_plan.pane_frame.x + metrics.pane_border_width + metrics.terminal_gutter_x
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
            metrics,
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
            metrics,
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
    fn pathological_horizontal_splits_collapse_to_renderable_portal_panes() {
        let core = SharedCore::bootstrap(bootstrap());
        for _ in 0..6 {
            core.split_with_terminal();
        }

        let snapshot = core.snapshot();
        let widths = snapshot
            .portal
            .panes
            .iter()
            .map(|plan| plan.pane_frame.width)
            .collect::<Vec<_>>();
        assert!(
            snapshot.portal.panes.len() < snapshot.current_workspace.pane_count,
            "expected tiny split branches to be elided from the rendered portal"
        );
        assert!(
            snapshot
                .portal
                .panes
                .iter()
                .all(|plan| { plan.pane_frame.width >= MIN_RENDERED_NATIVE_SURFACE_WIDTH_PX }),
            "unexpected rendered pane widths: {widths:?}"
        );
        let active_plan = snapshot
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == snapshot.current_workspace.active_pane)
            .expect("active pane should remain renderable");
        assert!(active_plan.pane_frame.width >= MIN_RENDERED_NATIVE_SURFACE_WIDTH_PX);
    }

    #[test]
    fn split_browser_creates_real_browser_pane() {
        let core = SharedCore::bootstrap(bootstrap());
        let before = core.snapshot().portal.panes.len();

        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::PersistentDefault,
        });

        let snapshot = core.snapshot();
        assert!(snapshot.portal.panes.len() > before);
        assert!(snapshot.portal.panes.iter().any(|plan| {
            matches!(
                &plan.mount,
                SurfaceMountSpec::Browser(BrowserMountSpec { url, .. })
                    if url == DEFAULT_BROWSER_HOME
            )
        }));
    }

    #[test]
    fn split_browser_preserves_requested_profile_mode() {
        let core = SharedCore::bootstrap(bootstrap());

        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::Ephemeral,
        });

        let snapshot = core.snapshot();
        let browser = snapshot.browser_chrome.expect("active browser chrome");
        assert_eq!(browser.profile_mode, BrowserProfileMode::Ephemeral);
        assert!(snapshot.portal.panes.iter().any(|plan| {
            matches!(
                &plan.mount,
                SurfaceMountSpec::Browser(BrowserMountSpec { profile_mode, .. })
                    if *profile_mode == BrowserProfileMode::Ephemeral
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
        let pane = find_pane(&snapshot.current_workspace.layout, browser_surface.pane_id)
            .expect("browser pane");

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
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::PersistentDefault,
        });

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
    fn resume_interrupted_agent_shell_action_queues_terminal_send_and_clears_prompt() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(|pane| pane.active_surface())
            .map(|surface| surface.id)
            .expect("surface");
        let surface = model
            .workspaces
            .get_mut(&workspace_id)
            .and_then(|workspace| workspace.panes.get_mut(&pane_id))
            .and_then(|pane| pane.surfaces.get_mut(&surface_id))
            .expect("surface");
        surface.metadata.cwd = Some("/tmp/taskers".into());
        surface.metadata.agent_kind = Some("codex".into());
        surface.metadata.agent_title = Some("Codex".into());
        surface.metadata.agent_command = Some("codex --model gpt-5".into());
        surface.attention = DomainAttentionState::WaitingInput;
        surface.interrupted_agent_resume = Some(InterruptedAgentResume {
            kind: "codex".into(),
            title: "Codex".into(),
            command: "codex --model gpt-5".into(),
            cwd: Some("/tmp/taskers".into()),
            captured_at: OffsetDateTime::now_utc(),
        });

        let core = SharedCore::bootstrap(BootstrapModel {
            app_state: preview_app_state_with_model(model, "taskers-preview-resume-action"),
            runtime_status: RuntimeStatus {
                ghostty_runtime: RuntimeCapability::Ready,
                shell_integration: RuntimeCapability::Ready,
                terminal_host: RuntimeCapability::Ready,
                terminal_persistence: RuntimeCapability::Ready,
            },
            selected_theme_id: "dark".into(),
            selected_shortcut_preset: super::ShortcutPreset::Balanced,
            configured_shell: None,
            embedded_terminal_settings: EmbeddedTerminalSettingsSnapshot::default(),
            notification_preferences: NotificationPreferencesSnapshot::default(),
            render_live_surfaces_in_overview: true,
            workspace_window_gap: DEFAULT_WORKSPACE_WINDOW_GAP,
        });

        core.dispatch_shell_action(ShellAction::ResumeInterruptedAgent {
            workspace_id,
            pane_id,
            surface_id,
        });

        assert_eq!(
            core.drain_host_commands(),
            vec![HostCommand::TerminalSendText {
                surface_id,
                text: "codex --model gpt-5\n".into(),
            }]
        );

        let snapshot = core.snapshot();
        let pane = find_pane(&snapshot.current_workspace.layout, pane_id).expect("pane");
        let surface = pane
            .surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
            .expect("surface");
        assert!(surface.interrupted_agent_resume.is_none());
        assert_eq!(surface.status_label, None);
    }

    #[test]
    fn browser_catalog_keeps_background_browser_surfaces() {
        let core = SharedCore::bootstrap(bootstrap());
        let first_workspace_id = core.snapshot().current_workspace.id;
        let first_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: Some(first_pane_id),
            profile_mode: BrowserProfileMode::PersistentDefault,
        });

        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let second_workspace_id = core.snapshot().current_workspace.id;
        let second_pane_id = core.snapshot().current_workspace.active_pane;
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: Some(second_pane_id),
            profile_mode: BrowserProfileMode::PersistentDefault,
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
    fn terminal_catalog_preserves_per_surface_env_for_inactive_tabs() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let workspace_id = snapshot.current_workspace.id;
        let pane_id = snapshot.current_workspace.active_pane;
        let first_surface_id = find_pane(&snapshot.current_workspace.layout, pane_id)
            .map(|pane| pane.active_surface)
            .expect("first surface");

        core.dispatch_shell_action(ShellAction::AddTerminalSurface {
            pane_id: Some(pane_id),
        });

        let snapshot = core.snapshot();
        let second_surface_id = find_pane(&snapshot.current_workspace.layout, pane_id)
            .map(|pane| pane.active_surface)
            .expect("second surface");
        assert_ne!(first_surface_id, second_surface_id);

        core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id: first_surface_id,
        });

        let catalog = core.snapshot().terminal_catalog;
        let background_entry = catalog
            .iter()
            .find(|entry| entry.surface_id == second_surface_id)
            .expect("background terminal entry");

        assert_eq!(background_entry.workspace_id, workspace_id);
        assert_eq!(background_entry.pane_id, pane_id);
        assert_eq!(
            background_entry.spec.env.get("TASKERS_WORKSPACE_ID"),
            Some(&workspace_id.to_string())
        );
        assert_eq!(
            background_entry.spec.env.get("TASKERS_PANE_ID"),
            Some(&pane_id.to_string())
        );
        assert_eq!(
            background_entry.spec.env.get("TASKERS_SURFACE_ID"),
            Some(&second_surface_id.to_string())
        );
    }

    #[test]
    fn browser_navigation_host_events_update_browser_chrome_snapshot() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::PersistentDefault,
        });

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
        core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: None,
            profile_mode: BrowserProfileMode::PersistentDefault,
        });

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
    fn redundant_host_pane_focus_event_does_not_advance_revision() {
        let core = SharedCore::bootstrap(bootstrap());
        let before = core.revision();
        let pane_id = core.snapshot().current_workspace.active_pane;
        let mut revisions = core.subscribe_revisions();
        revisions.borrow_and_update();

        core.apply_host_event(HostEvent::PaneFocused { pane_id });

        assert_eq!(core.revision(), before);
        assert_eq!(core.snapshot().current_workspace.active_pane, pane_id);
        assert!(!revisions.has_changed().expect("watch status"));
    }

    #[test]
    fn redundant_host_terminal_focus_event_does_not_advance_revision_when_tracking_is_empty() {
        let core = SharedCore::bootstrap(terminal_only_bootstrap());
        let before = core.revision();
        let pane_id = core.snapshot().current_workspace.active_pane;
        let mut revisions = core.subscribe_revisions();
        revisions.borrow_and_update();

        core.apply_host_event(HostEvent::PaneFocused { pane_id });

        assert_eq!(core.revision(), before);
        assert_eq!(core.snapshot().current_workspace.active_pane, pane_id);
        assert!(!revisions.has_changed().expect("watch status"));
    }

    #[test]
    fn host_pane_focus_event_for_other_workspace_still_notifies_subscribers() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspace);
        let target_workspace_id = core.snapshot().current_workspace.id;
        let target_pane_id = core.snapshot().current_workspace.active_pane;
        let original_workspace_id = core
            .snapshot()
            .workspaces
            .iter()
            .find(|workspace| workspace.id != target_workspace_id)
            .map(|workspace| workspace.id)
            .expect("original workspace");
        core.dispatch_shell_action(ShellAction::FocusWorkspace {
            workspace_id: original_workspace_id,
        });

        let before = core.revision();
        let mut revisions = core.subscribe_revisions();
        revisions.borrow_and_update();

        core.apply_host_event(HostEvent::PaneFocused {
            pane_id: target_pane_id,
        });

        assert!(core.revision() > before);
        assert_eq!(core.snapshot().current_workspace.id, target_workspace_id);
        assert!(revisions.has_changed().expect("watch status"));
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
    fn overview_mode_keeps_live_portal_surfaces_enabled_by_default() {
        let core = SharedCore::bootstrap(bootstrap());
        assert!(!core.snapshot().portal.panes.is_empty());

        core.dispatch_shell_action(ShellAction::ToggleOverview);

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert!(!snapshot.portal.panes.is_empty());
    }

    #[test]
    fn overview_mode_can_hide_live_portal_surfaces_when_disabled() {
        let core = SharedCore::bootstrap(bootstrap());

        core.dispatch_shell_action(ShellAction::SetOverviewLiveSurfaces { enabled: false });
        core.dispatch_shell_action(ShellAction::ToggleOverview);

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert!(snapshot.portal.panes.is_empty());
        assert!(!snapshot.settings.render_live_surfaces_in_overview);
    }

    #[test]
    fn workspace_window_gap_setting_updates_window_spacing() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let first_window_id = core.snapshot().current_workspace.active_window_id;

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let before = core.snapshot();
        let second_window_id = before.current_workspace.active_window_id;
        let first_window = window_snapshot(&before, first_window_id);
        let second_window = window_snapshot(&before, second_window_id);
        assert_eq!(
            second_window.frame.x - first_window.frame.right(),
            DEFAULT_WORKSPACE_WINDOW_GAP
        );

        core.dispatch_shell_action(ShellAction::SetWorkspaceWindowGap { gap: 24 });

        let after = core.snapshot();
        let first_window = window_snapshot(&after, first_window_id);
        let second_window = window_snapshot(&after, second_window_id);
        assert_eq!(after.settings.workspace_window_gap, 24);
        assert_eq!(second_window.frame.x - first_window.frame.right(), 24);
    }

    #[test]
    fn workspace_window_gap_setting_clamps_out_of_range_values() {
        let core = SharedCore::bootstrap(bootstrap());

        core.dispatch_shell_action(ShellAction::SetWorkspaceWindowGap { gap: -5 });
        assert_eq!(
            core.snapshot().settings.workspace_window_gap,
            MIN_WORKSPACE_WINDOW_GAP
        );

        core.dispatch_shell_action(ShellAction::SetWorkspaceWindowGap { gap: 999 });
        assert_eq!(
            core.snapshot().settings.workspace_window_gap,
            MAX_WORKSPACE_WINDOW_GAP
        );
    }

    #[test]
    fn workspace_window_gap_defaults_to_zero() {
        let core = SharedCore::bootstrap(bootstrap());
        assert_eq!(core.snapshot().settings.workspace_window_gap, 0);
    }

    #[test]
    fn configured_shell_setting_normalizes_blank_values() {
        let core = SharedCore::bootstrap(bootstrap());

        core.dispatch_shell_action(ShellAction::SetConfiguredShell {
            shell: Some("  /bin/fish  ".into()),
        });
        let snapshot = core.snapshot();
        assert_eq!(
            snapshot.settings.configured_shell.as_deref(),
            Some("/bin/fish")
        );
        assert!(!snapshot.settings.default_shell_label.is_empty());

        core.dispatch_shell_action(ShellAction::SetConfiguredShell {
            shell: Some("   ".into()),
        });
        assert_eq!(core.snapshot().settings.configured_shell, None);
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
        assert_eq!(
            active_window.frame.x,
            snapshot.portal.content.x + WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX
        );
        assert_eq!(active_window.frame.y, snapshot.portal.content.y);
        assert_eq!(
            active_window.frame.width,
            snapshot.portal.content.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2
        );
        assert_eq!(active_window.frame.height, snapshot.portal.content.height);
    }

    #[test]
    fn normal_mode_canvas_offsets_include_outer_resize_gutter() {
        let snapshot = SharedCore::bootstrap(bootstrap()).snapshot();

        assert_eq!(
            snapshot.current_workspace.canvas_offset_x,
            WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX
        );
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
    fn overview_scene_contains_one_card_per_workspace_window() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let snapshot = core.snapshot();
        let window_count = snapshot
            .current_workspace
            .columns
            .iter()
            .map(|column| column.windows.len())
            .sum::<usize>();

        assert_eq!(
            snapshot.current_workspace.overview_scene.cards.len(),
            window_count
        );
    }

    #[test]
    fn overview_scene_preview_lines_summarize_active_surfaces() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        let card = snapshot
            .current_workspace
            .overview_scene
            .cards
            .first()
            .expect("overview card");

        assert!(!card.preview_lines.is_empty());
        assert!(
            card.preview_lines
                .iter()
                .any(|line| line.contains("Terminal") || line.contains("Browser")),
            "expected preview lines to summarize active surfaces: {:?}",
            card.preview_lines
        );
    }

    #[test]
    fn overview_scene_preview_mode_tracks_live_preview_preference() {
        let core = SharedCore::bootstrap(bootstrap());
        let snapshot = core.snapshot();
        assert!(
            snapshot
                .current_workspace
                .overview_scene
                .cards
                .iter()
                .all(|card| matches!(
                    card.preview_mode,
                    super::OverviewPreviewModeSnapshot::LivePreferred
                ))
        );

        core.dispatch_shell_action(ShellAction::SetOverviewLiveSurfaces { enabled: false });
        let snapshot = core.snapshot();
        assert!(
            snapshot
                .current_workspace
                .overview_scene
                .cards
                .iter()
                .all(|card| matches!(
                    card.preview_mode,
                    super::OverviewPreviewModeSnapshot::Summary
                ))
        );
    }

    #[test]
    fn overview_scene_move_capabilities_follow_workspace_topology() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });

        let snapshot = core.snapshot();
        let cards = &snapshot.current_workspace.overview_scene.cards;
        let movable = cards
            .iter()
            .find(|card| {
                card.can_move_left || card.can_move_right || card.can_move_up || card.can_move_down
            })
            .expect("movable card");

        assert!(
            movable.can_move_left
                || movable.can_move_right
                || movable.can_move_up
                || movable.can_move_down
        );
    }

    #[test]
    fn presets_include_ctrl_alt_horizontal_split_resize_bindings() {
        assert_eq!(
            ShortcutAction::ResizeSplitLeft.accelerators(ShortcutPreset::Balanced),
            &["<Control><Alt>minus", "<Control><Alt>KP_Subtract"]
        );
        assert_eq!(
            ShortcutAction::ResizeSplitRight.accelerators(ShortcutPreset::Balanced),
            &["<Control><Alt>equal", "<Control><Alt>KP_Add",]
        );
        assert_eq!(
            ShortcutAction::ResizeSplitLeft.accelerators(ShortcutPreset::PowerUser),
            &[
                "<Control><Alt>minus",
                "<Control><Alt>KP_Subtract",
                "<Control><Alt><Shift>Home",
            ]
        );
        assert_eq!(
            ShortcutAction::ResizeSplitRight.accelerators(ShortcutPreset::PowerUser),
            &[
                "<Control><Alt>equal",
                "<Control><Alt>KP_Add",
                "<Control><Alt><Shift>End",
            ]
        );
        assert_eq!(
            ShortcutAction::FitTerminalToViewport.accelerators(ShortcutPreset::Balanced),
            &["<Control><Alt>0", "<Control><Alt>KP_0"]
        );
        assert_eq!(
            ShortcutAction::FitTerminalToViewport.accelerators(ShortcutPreset::PowerUser),
            &["<Control><Alt>0", "<Control><Alt>KP_0"]
        );
    }

    #[test]
    fn focused_split_ratio_targets_follow_active_leaf_path() {
        let first = PaneId::new();
        let second = PaneId::new();
        let target = PaneId::new();
        let layout = taskers_domain::SplitLayoutNode::Split {
            axis: taskers_domain::SplitAxis::Horizontal,
            ratio: 500,
            first: Box::new(taskers_domain::SplitLayoutNode::Leaf { leaf_id: first }),
            second: Box::new(taskers_domain::SplitLayoutNode::Split {
                axis: taskers_domain::SplitAxis::Vertical,
                ratio: 500,
                first: Box::new(taskers_domain::SplitLayoutNode::Leaf { leaf_id: target }),
                second: Box::new(taskers_domain::SplitLayoutNode::Leaf { leaf_id: second }),
            }),
        };

        assert_eq!(
            focused_split_ratio_targets(&layout, target),
            vec![
                (vec![true], EXPANDED_ACTIVE_SPLIT_RATIO),
                (Vec::new(), COLLAPSED_INACTIVE_SPLIT_RATIO),
            ]
        );
    }

    #[test]
    fn new_window_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::NewWindowRight));

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert_eq!(snapshot.current_workspace.columns.len(), 2);
    }

    #[test]
    fn new_window_right_shortcut_splits_active_window_width_in_half() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let before = core.snapshot();
        let source_window_id = before.current_workspace.active_window_id;
        let source_window = window_snapshot(&before, source_window_id);
        let expected_width = ((source_window.frame.width - DEFAULT_WORKSPACE_WINDOW_GAP).max(2)
            / 2)
        .max(MIN_WORKSPACE_WINDOW_WIDTH);

        assert!(core.dispatch_shortcut_action(ShortcutAction::NewWindowRight));

        let after = core.snapshot();
        let moved_source_window = window_snapshot(&after, source_window_id);
        let new_window = window_snapshot(&after, after.current_workspace.active_window_id);
        assert_eq!(moved_source_window.frame.width, expected_width);
        assert_eq!(new_window.frame.width, expected_width);
    }

    #[test]
    fn new_window_down_shortcut_splits_active_window_height_in_half() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let before = core.snapshot();
        let source_window_id = before.current_workspace.active_window_id;
        let source_window = window_snapshot(&before, source_window_id);
        let expected_height = ((source_window.frame.height - DEFAULT_WORKSPACE_WINDOW_GAP).max(2)
            / 2)
        .max(MIN_WORKSPACE_WINDOW_HEIGHT);

        assert!(core.dispatch_shortcut_action(ShortcutAction::NewWindowDown));

        let after = core.snapshot();
        let moved_source_window = window_snapshot(&after, source_window_id);
        let new_window = window_snapshot(&after, after.current_workspace.active_window_id);
        assert_eq!(moved_source_window.frame.height, expected_height);
        assert_eq!(new_window.frame.height, expected_height);
    }

    #[test]
    fn close_terminal_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::CloseTerminal));

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert_eq!(snapshot.current_workspace.columns.len(), 1);
    }

    #[test]
    fn move_window_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let before = core.snapshot();
        let active_window_id = before.current_workspace.active_window_id;
        let before_column_index = before
            .current_workspace
            .columns
            .iter()
            .position(|column| {
                column
                    .windows
                    .iter()
                    .any(|window| window.id == active_window_id)
            })
            .expect("window column index");
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::MoveWindowLeft));

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        let after_column_index = snapshot
            .current_workspace
            .columns
            .iter()
            .position(|column| {
                column
                    .windows
                    .iter()
                    .any(|window| window.id == active_window_id)
            })
            .expect("window column index");
        assert_ne!(after_column_index, before_column_index);
    }

    #[test]
    fn resize_window_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let before = core.snapshot();
        let active_window_id = before.current_workspace.active_window_id;
        let before_width = window_snapshot(&before, active_window_id).frame.width;
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::ResizeWindowLeft));

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        let after_width = window_snapshot(&snapshot, active_window_id).frame.width;
        assert_ne!(after_width, before_width);
    }

    #[test]
    fn fit_terminal_to_viewport_shortcut_expands_active_terminal_and_window() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Down,
        });
        assert!(core.dispatch_shortcut_action(ShortcutAction::SplitRight));

        let before = core.snapshot();
        let active_window_id = before.current_workspace.active_window_id;
        let active_pane_id = before.current_workspace.active_pane;
        let metrics = before.metrics;
        let before_window = window_snapshot(&before, active_window_id);
        let before_pane_frame = find_pane_frame(
            &before_window.layout,
            active_pane_id,
            workspace_window_content_frame(before_window.frame, metrics),
            metrics,
        )
        .expect("active pane frame before fit");

        assert!(core.dispatch_shortcut_action(ShortcutAction::FitTerminalToViewport));

        let after = core.snapshot();
        let after_window = window_snapshot(&after, active_window_id);
        let active_pane = find_pane(&after.current_workspace.layout, active_pane_id).expect("pane");
        let active_plan = after
            .portal
            .panes
            .iter()
            .find(|plan| plan.pane_id == active_pane_id)
            .expect("active portal plan");
        let expected_window_width =
            after.portal.content.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2;
        let expected_window_height = after.portal.content.height;
        let expected_pane_frame = pane_container_content_frame(
            workspace_window_content_frame(after_window.frame, metrics),
            metrics,
        );

        assert!(before_window.frame.height < expected_window_height);
        assert!(before_pane_frame.width < expected_pane_frame.width);
        assert!(before_pane_frame.height <= expected_pane_frame.height);
        assert_eq!(after_window.frame.width, expected_window_width);
        assert_eq!(after_window.frame.height, expected_window_height);
        assert_eq!(
            active_plan.frame,
            pane_body_frame(
                expected_pane_frame,
                metrics,
                &taskers_domain::PaneKind::Terminal,
                pane_shows_tab_strip_for_surface_count(active_pane.surfaces.len()),
            )
        );
    }

    #[test]
    fn fit_terminal_to_viewport_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::FitTerminalToViewport));

        assert!(core.snapshot().overview_mode);
    }

    #[test]
    fn horizontal_resize_shortcut_falls_back_to_window_width_without_split() {
        let core = SharedCore::bootstrap(terminal_only_bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        let before = core.snapshot();
        let active_window_id = before.current_workspace.active_window_id;
        let before_width = window_snapshot(&before, active_window_id).frame.width;
        let before_model_width = {
            let guard = core.inner.lock();
            let model = guard.app_state.snapshot_model();
            let workspace = model.active_workspace().expect("workspace");
            let column_id = workspace.active_column_id().expect("active column");
            workspace
                .columns
                .get(&column_id)
                .expect("active column record")
                .width
        };

        assert!(core.dispatch_shortcut_action(ShortcutAction::ResizeSplitRight));

        let after = core.snapshot();
        let after_width = window_snapshot(&after, active_window_id).frame.width;
        let after_model_width = {
            let guard = core.inner.lock();
            let model = guard.app_state.snapshot_model();
            let workspace = model.active_workspace().expect("workspace");
            let column_id = workspace.active_column_id().expect("active column");
            workspace
                .columns
                .get(&column_id)
                .expect("active column record")
                .width
        };
        assert!(
            after_model_width > before_model_width,
            "expected stored column width to grow; before={before_model_width}, after={after_model_width}"
        );
        assert!(
            after_width > before_width,
            "expected active window width to grow when no split can resize; before={before_width}, after={after_width}, stored_before={before_model_width}, stored_after={after_model_width}"
        );
    }

    #[test]
    fn horizontal_resize_shortcut_prefers_split_width_when_available() {
        let core = SharedCore::bootstrap(bootstrap());
        assert!(core.dispatch_shortcut_action(ShortcutAction::SplitRight));

        let before = core.snapshot();
        let active_window_id = before.current_workspace.active_window_id;
        let active_pane_id = before.current_workspace.active_pane;
        let metrics = LayoutMetrics::default();
        let before_window = window_snapshot(&before, active_window_id);
        let before_window_width = before_window.frame.width;
        let before_pane_width = find_pane_frame(
            &before_window.layout,
            active_pane_id,
            workspace_window_content_frame(before_window.frame, metrics),
            metrics,
        )
        .expect("active pane frame before resize")
        .width;

        assert!(core.dispatch_shortcut_action(ShortcutAction::ResizeSplitRight));

        let after = core.snapshot();
        let after_window = window_snapshot(&after, active_window_id);
        let after_pane_width = find_pane_frame(
            &after_window.layout,
            active_pane_id,
            workspace_window_content_frame(after_window.frame, metrics),
            metrics,
        )
        .expect("active pane frame after resize")
        .width;

        assert_eq!(after_window.frame.width, before_window_width);
        assert!(
            after_pane_width > before_pane_width,
            "expected active pane width to grow by resizing the split"
        );
    }

    #[test]
    fn focus_window_shortcut_keeps_overview_mode_active() {
        let core = SharedCore::bootstrap(bootstrap());
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let before = core.snapshot();
        let before_window_id = before.current_workspace.active_window_id;
        core.dispatch_shell_action(ShellAction::ToggleOverview);
        assert!(core.snapshot().overview_mode);

        assert!(core.dispatch_shortcut_action(ShortcutAction::FocusLeft));

        let snapshot = core.snapshot();
        assert!(snapshot.overview_mode);
        assert_ne!(
            snapshot.current_workspace.active_window_id,
            before_window_id
        );
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
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });
        let workspace_id = core.snapshot().current_workspace.id;
        let widths = core
            .snapshot()
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, 720))
            .collect::<Vec<_>>();
        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths,
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);
        core.dispatch_shell_action(ShellAction::ScrollViewport { dx: -50_000, dy: 0 });
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
    fn creating_workspace_window_scrolls_new_active_window_into_view() {
        let core = SharedCore::bootstrap(bootstrap());
        core.set_window_size(PixelSize::new(1280, 900));
        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let before = core.snapshot();
        let workspace_id = before.current_workspace.id;
        let widths = before
            .current_workspace
            .columns
            .iter()
            .map(|column| (column.id, 720))
            .collect::<Vec<_>>();
        core.dispatch_shell_action(ShellAction::PreviewResize {
            preview: ResizePreview::WorkspaceColumnWidths {
                workspace_id,
                widths,
            },
        });
        core.dispatch_shell_action(ShellAction::CommitResizePreview);

        core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
            direction: WorkspaceDirection::Right,
        });

        let snapshot = core.snapshot();
        let active_window = window_snapshot(&snapshot, snapshot.current_workspace.active_window_id);
        let visible_left = snapshot.current_workspace.viewport_origin_x;
        let visible_right =
            visible_left + snapshot.portal.content.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX;

        assert!(
            snapshot.current_workspace.viewport_x > 0,
            "expected viewport to scroll toward the newly focused window"
        );
        assert!(
            active_window.frame.x >= visible_left,
            "expected active window left edge to be visible"
        );
        if active_window.frame.width
            <= snapshot.portal.content.width - WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX * 2
        {
            assert!(
                active_window.frame.right() <= visible_right,
                "expected active window right edge to be visible"
            );
        } else {
            assert_eq!(
                visible_left + WORKSPACE_OUTER_EDGE_RESIZE_GUTTER_PX,
                active_window.frame.x,
                "expected oversized active window to align its left edge with the viewport"
            );
        }
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
            profile_mode: BrowserProfileMode::PersistentDefault,
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
    fn focusing_browser_surface_keeps_vcs_target_on_last_terminal() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let terminal_surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("terminal surface");
        let browser_surface_id = model
            .create_surface(workspace_id, pane_id, taskers_domain::PaneKind::Browser)
            .expect("browser surface");

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-vcs-terminal-target",
        ));
        core.dispatch_shell_action(ShellAction::ToggleVcsPanel);
        core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id: terminal_surface_id,
        });
        core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id: browser_surface_id,
        });

        let snapshot = core.snapshot();
        assert_eq!(
            snapshot.vcs_panel.target_surface_id,
            Some(terminal_surface_id)
        );
    }

    #[test]
    fn vcs_panel_reads_git_stats_and_unpushed_commits_from_repo_target() {
        let fixture = init_git_vcs_fixture();
        let model = model_with_terminal_cwd(&fixture.path().join("repo"));
        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-vcs-git-refresh",
        ));

        core.dispatch_shell_action(ShellAction::ToggleVcsPanel);

        let snapshot = core.snapshot();
        assert!(snapshot.vcs_panel.visible);
        let vcs = snapshot.vcs_panel.snapshot.expect("vcs snapshot");
        assert_eq!(vcs.mode, super::VcsMode::Git);
        assert_eq!(vcs.total_insertions, 1);
        assert_eq!(vcs.total_deletions, 0);
        assert_eq!(vcs.recent_commits.len(), 1);
        assert_eq!(
            vcs.recent_commits[0].description,
            "feat: add unpushed change"
        );
        let file = vcs
            .files
            .iter()
            .find(|file| file.path == "working.txt")
            .expect("working tree file");
        assert_eq!(file.insertions, Some(1));
        assert_eq!(file.deletions, Some(0));
    }

    #[test]
    fn showing_vcs_diff_loads_preview_for_selected_git_file() {
        let fixture = init_git_vcs_fixture();
        let model = model_with_terminal_cwd(&fixture.path().join("repo"));
        let core =
            SharedCore::bootstrap(bootstrap_with_model(model, "taskers-preview-vcs-git-diff"));

        core.dispatch_shell_action(ShellAction::ToggleVcsPanel);
        core.dispatch_shell_action(ShellAction::ShowVcsDiff {
            path: Some("working.txt".into()),
        });

        let snapshot = core.snapshot();
        let vcs = snapshot.vcs_panel.snapshot.expect("vcs snapshot");
        assert_eq!(vcs.diff_path.as_deref(), Some("working.txt"));
        let diff = vcs.diff_text.expect("diff text");
        assert!(diff.contains("working tree change"));
        assert!(diff.contains("+++"));
    }

    #[test]
    fn vcs_panel_reads_jj_stats_and_unpushed_commits_from_repo_target() {
        let Some(fixture) = init_jj_vcs_fixture() else {
            return;
        };
        let model = model_with_terminal_cwd(&fixture.path().join("repo"));
        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-vcs-jj-refresh",
        ));

        core.dispatch_shell_action(ShellAction::ToggleVcsPanel);

        let snapshot = core.snapshot();
        assert!(snapshot.vcs_panel.visible);
        let vcs = snapshot.vcs_panel.snapshot.expect("vcs snapshot");
        assert_eq!(vcs.mode, super::VcsMode::Jj);
        assert_eq!(vcs.total_insertions, 2);
        assert_eq!(vcs.total_deletions, 0);
        assert_eq!(vcs.recent_commits.len(), 1);
        assert_eq!(
            vcs.recent_commits[0].description,
            "feat: add unpushed change"
        );
        let working_file = vcs
            .files
            .iter()
            .find(|file| file.path == "working.txt")
            .expect("working tree file");
        assert_eq!(working_file.insertions, Some(1));
        assert_eq!(working_file.deletions, Some(0));
        let committed_file = vcs
            .files
            .iter()
            .find(|file| file.path == "committed.txt")
            .expect("committed file");
        assert_eq!(committed_file.insertions, Some(1));
        assert_eq!(committed_file.deletions, Some(0));
    }

    #[test]
    fn showing_vcs_diff_loads_preview_for_selected_jj_file() {
        let Some(fixture) = init_jj_vcs_fixture() else {
            return;
        };
        let model = model_with_terminal_cwd(&fixture.path().join("repo"));
        let core =
            SharedCore::bootstrap(bootstrap_with_model(model, "taskers-preview-vcs-jj-diff"));

        core.dispatch_shell_action(ShellAction::ToggleVcsPanel);
        core.dispatch_shell_action(ShellAction::ShowVcsDiff {
            path: Some("working.txt".into()),
        });

        let snapshot = core.snapshot();
        let vcs = snapshot.vcs_panel.snapshot.expect("vcs snapshot");
        assert_eq!(vcs.diff_path.as_deref(), Some("working.txt"));
        let diff = vcs.diff_text.expect("diff text");
        assert!(diff.contains("Modified regular file working.txt"));
        assert!(diff.contains("working tree change"));
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
    fn dismiss_activity_clears_notification_ring_outline_state() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface");

        model
            .create_agent_notification(
                taskers_domain::AgentTarget::Surface {
                    workspace_id,
                    pane_id,
                    surface_id,
                },
                taskers_domain::SignalKind::Notification,
                Some("Codex".into()),
                None,
                None,
                "Need input".into(),
                taskers_domain::AttentionState::WaitingInput,
            )
            .expect("notification");

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-dismiss-activity-ring",
        ));

        let before = core.snapshot();
        let pane = find_pane(
            &before.current_workspace.layout,
            before.current_workspace.active_pane,
        )
        .expect("pane before dismiss");
        assert_eq!(
            pane.notification_ring,
            Some(super::AttentionRingState::Waiting)
        );

        let activity_id = before
            .activity
            .first()
            .map(|item| item.id)
            .expect("notification activity");

        core.dispatch_shell_action(ShellAction::DismissActivity { activity_id });

        let after = core.snapshot();
        let pane = find_pane(
            &after.current_workspace.layout,
            after.current_workspace.active_pane,
        )
        .expect("pane after dismiss");
        assert_eq!(pane.notification_ring, None);
        assert!(
            pane.surfaces
                .iter()
                .all(|surface| surface.notification_ring.is_none())
        );
    }

    #[test]
    fn dismiss_surface_alert_removes_working_agent_session_and_status() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface");

        model
            .start_surface_agent_session(workspace_id, pane_id, surface_id, "codex".into())
            .expect("working session");
        model
            .apply_surface_signal(
                workspace_id,
                pane_id,
                surface_id,
                taskers_domain::SignalEvent::with_metadata(
                    "agent-hook:codex",
                    taskers_domain::SignalKind::Started,
                    Some("Working".into()),
                    Some(taskers_domain::SignalPaneMetadata {
                        title: None,
                        agent_title: Some("Codex".into()),
                        cwd: None,
                        repo_name: None,
                        git_branch: None,
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(true),
                        agent_command: None,
                    }),
                ),
            )
            .expect("started signal");

        let core = SharedCore::bootstrap(bootstrap_with_model(
            model,
            "taskers-preview-dismiss-surface-alert",
        ));
        assert_eq!(core.snapshot().agents.len(), 1);

        core.dispatch_shell_action(ShellAction::DismissSurfaceAlert {
            workspace_id,
            pane_id,
            surface_id,
        });

        let snapshot = core.snapshot();
        assert!(snapshot.agents.is_empty());
        let pane = find_pane(&snapshot.current_workspace.layout, pane_id).expect("pane");
        let surface = pane
            .surfaces
            .iter()
            .find(|surface| surface.id == surface_id)
            .expect("surface");
        assert_eq!(surface.status_label, None);
        assert_eq!(surface.notification_ring, None);
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
