use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use taskers_domain::{
    AgentTarget, AppModel, AttentionState, Direction, NotificationDeliveryState, NotificationId,
    PaneId, PaneKind, PaneMetadataPatch, PersistedSession, ProgressState, SignalEvent, SignalKind,
    SplitAxis, SurfaceId, WindowId, WorkspaceColumnId, WorkspaceId, WorkspaceLogEntry,
    WorkspaceViewport, WorkspaceWindowId, WorkspaceWindowMoveTarget, WorkspaceWindowTabId,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum ControlCommand {
    CreateWorkspace {
        label: String,
    },
    RenameWorkspace {
        workspace_id: WorkspaceId,
        label: String,
    },
    SwitchWorkspace {
        window_id: Option<WindowId>,
        workspace_id: WorkspaceId,
    },
    SplitPane {
        workspace_id: WorkspaceId,
        pane_id: Option<PaneId>,
        axis: SplitAxis,
    },
    SplitPaneDirection {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        direction: Direction,
    },
    CreateWorkspaceWindow {
        workspace_id: WorkspaceId,
        direction: Direction,
    },
    FocusWorkspaceWindow {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
    },
    MoveWorkspaceWindow {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        target: WorkspaceWindowMoveTarget,
    },
    CreateWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
    },
    FocusWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        workspace_window_tab_id: WorkspaceWindowTabId,
    },
    MoveWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        workspace_window_tab_id: WorkspaceWindowTabId,
        to_index: usize,
    },
    TransferWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        source_workspace_window_id: WorkspaceWindowId,
        workspace_window_tab_id: WorkspaceWindowTabId,
        target_workspace_window_id: WorkspaceWindowId,
        to_index: usize,
    },
    ExtractWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        source_workspace_window_id: WorkspaceWindowId,
        workspace_window_tab_id: WorkspaceWindowTabId,
        target: WorkspaceWindowMoveTarget,
    },
    CloseWorkspaceWindowTab {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        workspace_window_tab_id: WorkspaceWindowTabId,
    },
    FocusPane {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    },
    FocusPaneDirection {
        workspace_id: WorkspaceId,
        direction: Direction,
    },
    ResizeActiveWindow {
        workspace_id: WorkspaceId,
        direction: Direction,
        amount: i32,
    },
    ResizeActivePaneSplit {
        workspace_id: WorkspaceId,
        direction: Direction,
        amount: i32,
    },
    SetWorkspaceColumnWidth {
        workspace_id: WorkspaceId,
        workspace_column_id: WorkspaceColumnId,
        width: i32,
    },
    SetWorkspaceWindowHeight {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        height: i32,
    },
    SetWindowSplitRatio {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        path: Vec<bool>,
        ratio: u16,
    },
    UpdatePaneMetadata {
        pane_id: PaneId,
        patch: PaneMetadataPatch,
    },
    UpdateSurfaceMetadata {
        surface_id: SurfaceId,
        patch: PaneMetadataPatch,
    },
    CreateSurface {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        kind: PaneKind,
    },
    FocusSurface {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    StartSurfaceAgentSession {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
        agent_kind: String,
    },
    StopSurfaceAgentSession {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
        exit_status: i32,
    },
    MarkSurfaceCompleted {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    CloseSurface {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    MoveSurface {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
        to_index: usize,
    },
    TransferSurface {
        source_workspace_id: WorkspaceId,
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_workspace_id: WorkspaceId,
        target_pane_id: PaneId,
        to_index: usize,
    },
    MoveSurfaceToSplit {
        source_workspace_id: WorkspaceId,
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_workspace_id: WorkspaceId,
        target_pane_id: PaneId,
        direction: Direction,
    },
    MoveSurfaceToWorkspace {
        source_workspace_id: WorkspaceId,
        source_pane_id: PaneId,
        surface_id: SurfaceId,
        target_workspace_id: WorkspaceId,
    },
    SetWorkspaceViewport {
        workspace_id: WorkspaceId,
        viewport: WorkspaceViewport,
    },
    ClosePane {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    },
    CloseWorkspace {
        workspace_id: WorkspaceId,
    },
    ReorderWorkspaces {
        window_id: WindowId,
        workspace_ids: Vec<WorkspaceId>,
    },
    EmitSignal {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: Option<SurfaceId>,
        event: SignalEvent,
    },
    AgentSetStatus {
        workspace_id: WorkspaceId,
        text: String,
    },
    AgentClearStatus {
        workspace_id: WorkspaceId,
    },
    AgentSetProgress {
        workspace_id: WorkspaceId,
        progress: ProgressState,
    },
    AgentClearProgress {
        workspace_id: WorkspaceId,
    },
    AgentAppendLog {
        workspace_id: WorkspaceId,
        entry: WorkspaceLogEntry,
    },
    AgentClearLog {
        workspace_id: WorkspaceId,
    },
    AgentCreateNotification {
        target: AgentTarget,
        kind: SignalKind,
        title: Option<String>,
        subtitle: Option<String>,
        external_id: Option<String>,
        message: String,
        state: AttentionState,
    },
    OpenNotification {
        window_id: Option<WindowId>,
        notification_id: NotificationId,
    },
    ClearNotification {
        notification_id: NotificationId,
    },
    MarkNotificationDelivery {
        notification_id: NotificationId,
        delivery: NotificationDeliveryState,
    },
    AgentClearNotifications {
        target: AgentTarget,
    },
    DismissSurfaceAlert {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    DismissInterruptedAgentResume {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    AgentTriggerFlash {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    AgentFocusLatestUnread {
        window_id: Option<WindowId>,
    },
    Browser {
        browser_command: BrowserControlCommand,
    },
    TerminalDebug {
        debug_command: TerminalDebugCommand,
    },
    Vcs {
        vcs_command: VcsCommand,
    },
    QueryStatus {
        query: ControlQuery,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "vcs_command", rename_all = "snake_case")]
pub enum VcsCommand {
    Refresh {
        surface_id: SurfaceId,
        diff_path: Option<String>,
    },
    GitCommit {
        surface_id: SurfaceId,
        message: String,
    },
    GitCreateBranch {
        surface_id: SurfaceId,
        name: String,
    },
    GitSwitchBranch {
        surface_id: SurfaceId,
        name: String,
    },
    GitFetch {
        surface_id: SurfaceId,
    },
    GitPull {
        surface_id: SurfaceId,
    },
    GitPush {
        surface_id: SurfaceId,
    },
    JjDescribe {
        surface_id: SurfaceId,
        message: String,
    },
    JjNew {
        surface_id: SurfaceId,
        message: Option<String>,
    },
    JjCreateBookmark {
        surface_id: SurfaceId,
        name: String,
    },
    JjSwitchBookmark {
        surface_id: SurfaceId,
        name: String,
    },
    JjFetch {
        surface_id: SurfaceId,
    },
    JjPush {
        surface_id: SurfaceId,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VcsMode {
    Git,
    Jj,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VcsFileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    Untracked,
    Conflicted,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsFileEntry {
    pub path: String,
    pub status: VcsFileStatus,
    pub staged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsRefEntry {
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsPullRequestInfo {
    pub number: Option<u32>,
    pub title: Option<String>,
    pub url: String,
    pub state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsSnapshot {
    pub surface_id: SurfaceId,
    pub mode: VcsMode,
    pub repo_root: String,
    pub repo_name: String,
    pub cwd: String,
    pub headline: String,
    pub detail: Option<String>,
    pub summary_text: String,
    pub files: Vec<VcsFileEntry>,
    pub refs: Vec<VcsRefEntry>,
    pub diff_path: Option<String>,
    pub diff_text: Option<String>,
    pub pull_request: Option<VcsPullRequestInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VcsCommandResult {
    pub snapshot: Option<VcsSnapshot>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlErrorCode {
    InvalidParams,
    NotFound,
    Timeout,
    InvalidState,
    NotSupported,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlError {
    pub code: ControlErrorCode,
    pub message: String,
}

impl ControlError {
    pub fn new(code: ControlErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::InvalidParams, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::NotFound, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::Timeout, message)
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::InvalidState, message)
    }

    pub fn not_supported(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::NotSupported, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ControlErrorCode::Internal, message)
    }
}

impl std::fmt::Display for ControlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for ControlError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "browser_command", rename_all = "snake_case")]
pub enum BrowserControlCommand {
    Navigate {
        surface_id: SurfaceId,
        url: String,
    },
    Back {
        surface_id: SurfaceId,
    },
    Forward {
        surface_id: SurfaceId,
    },
    Reload {
        surface_id: SurfaceId,
    },
    FocusWebview {
        surface_id: SurfaceId,
    },
    IsWebviewFocused {
        surface_id: SurfaceId,
    },
    Snapshot {
        surface_id: SurfaceId,
    },
    Eval {
        surface_id: SurfaceId,
        script: String,
    },
    Wait {
        surface_id: SurfaceId,
        condition: BrowserWaitCondition,
        timeout_ms: u64,
        poll_interval_ms: u64,
    },
    Click {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Dblclick {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Type {
        surface_id: SurfaceId,
        target: BrowserTarget,
        text: String,
        snapshot_after: bool,
    },
    Fill {
        surface_id: SurfaceId,
        target: BrowserTarget,
        text: String,
        snapshot_after: bool,
    },
    Press {
        surface_id: SurfaceId,
        target: Option<BrowserTarget>,
        key: String,
        snapshot_after: bool,
    },
    Keydown {
        surface_id: SurfaceId,
        target: Option<BrowserTarget>,
        key: String,
        snapshot_after: bool,
    },
    Keyup {
        surface_id: SurfaceId,
        target: Option<BrowserTarget>,
        key: String,
        snapshot_after: bool,
    },
    Hover {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Focus {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Check {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Uncheck {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Select {
        surface_id: SurfaceId,
        target: BrowserTarget,
        values: Vec<String>,
        snapshot_after: bool,
    },
    Scroll {
        surface_id: SurfaceId,
        target: Option<BrowserTarget>,
        dx: i32,
        dy: i32,
        snapshot_after: bool,
    },
    ScrollIntoView {
        surface_id: SurfaceId,
        target: BrowserTarget,
        snapshot_after: bool,
    },
    Get {
        surface_id: SurfaceId,
        query: BrowserGetCommand,
    },
    Is {
        surface_id: SurfaceId,
        query: BrowserPredicateCommand,
    },
    Screenshot {
        surface_id: SurfaceId,
        path: Option<String>,
        full_document: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "snake_case")]
pub enum BrowserTarget {
    Ref { value: String },
    Selector { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "condition", rename_all = "snake_case")]
pub enum BrowserWaitCondition {
    Selector { selector: String },
    Text { text: String },
    UrlMatches { pattern: String },
    LoadState { state: BrowserLoadState },
    Function { script: String },
    Delay { duration_ms: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserLoadState {
    Started,
    Redirected,
    Committed,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum BrowserGetCommand {
    Url,
    Title,
    Text {
        target: BrowserTarget,
    },
    Html {
        target: BrowserTarget,
    },
    Value {
        target: BrowserTarget,
    },
    Attr {
        target: BrowserTarget,
        name: String,
    },
    Count {
        selector: String,
    },
    Box {
        target: BrowserTarget,
    },
    Styles {
        target: BrowserTarget,
        properties: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "query", rename_all = "snake_case")]
pub enum BrowserPredicateCommand {
    Visible { target: BrowserTarget },
    Enabled { target: BrowserTarget },
    Checked { target: BrowserTarget },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "terminal_command", rename_all = "snake_case")]
pub enum TerminalDebugCommand {
    IsFocused {
        surface_id: SurfaceId,
    },
    ReadText {
        surface_id: SurfaceId,
        tail_lines: Option<usize>,
    },
    RenderStats {
        surface_id: SurfaceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum TerminalDebugResult {
    IsFocused { focused: bool },
    ReadText { text: String },
    RenderStats { stats: TerminalRenderStats },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRenderStats {
    pub surface_id: SurfaceId,
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub mounted: bool,
    pub visible: bool,
    pub focused: bool,
    pub backend: String,
    pub cols: u16,
    pub rows: u16,
    pub width_px: i32,
    pub height_px: i32,
    pub has_selection: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentifyContext {
    pub window_id: WindowId,
    pub workspace_id: WorkspaceId,
    pub workspace_label: String,
    pub workspace_window_id: Option<WorkspaceWindowId>,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub surface_kind: PaneKind,
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub loading: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentifyResult {
    pub focused: IdentifyContext,
    pub caller: Option<IdentifyContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ControlQuery {
    ActiveWindow,
    Window {
        window_id: WindowId,
    },
    Workspace {
        workspace_id: WorkspaceId,
    },
    Identify {
        workspace_id: Option<WorkspaceId>,
        pane_id: Option<PaneId>,
        surface_id: Option<SurfaceId>,
    },
    All,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ControlResponse {
    Ack {
        message: String,
    },
    WorkspaceCreated {
        workspace_id: WorkspaceId,
    },
    PaneSplit {
        pane_id: PaneId,
    },
    SurfaceMovedToSplit {
        pane_id: PaneId,
    },
    SurfaceMovedToWorkspace {
        pane_id: PaneId,
    },
    SurfaceCreated {
        surface_id: SurfaceId,
    },
    WorkspaceWindowCreated {
        pane_id: PaneId,
    },
    WorkspaceWindowTabCreated {
        pane_id: PaneId,
        workspace_window_tab_id: WorkspaceWindowTabId,
    },
    Status {
        session: PersistedSession,
    },
    WorkspaceState {
        workspace_id: WorkspaceId,
        session: PersistedSession,
    },
    Browser {
        result: JsonValue,
    },
    TerminalDebug {
        result: TerminalDebugResult,
    },
    Vcs {
        result: VcsCommandResult,
    },
    Identify {
        result: IdentifyResult,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestFrame {
    pub request_id: Uuid,
    pub command: ControlCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseFrame {
    pub request_id: Uuid,
    pub response: Result<ControlResponse, ControlError>,
}

impl RequestFrame {
    pub fn new(command: ControlCommand) -> Self {
        Self {
            request_id: Uuid::now_v7(),
            command,
        }
    }
}

impl From<AppModel> for ControlResponse {
    fn from(model: AppModel) -> Self {
        Self::Status {
            session: model.snapshot(),
        }
    }
}
