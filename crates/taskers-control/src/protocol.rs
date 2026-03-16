use serde::{Deserialize, Serialize};
use uuid::Uuid;

use taskers_domain::{
    AppModel, Direction, PaneId, PaneKind, PaneMetadataPatch, PersistedSession, SignalEvent,
    SplitAxis, SurfaceId, WindowId, WorkspaceColumnId, WorkspaceId, WorkspaceViewport,
    WorkspaceWindowId,
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
    CreateWorkspaceWindow {
        workspace_id: WorkspaceId,
        direction: Direction,
    },
    FocusWorkspaceWindow {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
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
    EmitSignal {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: Option<SurfaceId>,
        event: SignalEvent,
    },
    QueryStatus {
        query: ControlQuery,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ControlQuery {
    ActiveWindow,
    Window { window_id: WindowId },
    Workspace { workspace_id: WorkspaceId },
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
    SurfaceCreated {
        surface_id: SurfaceId,
    },
    WorkspaceWindowCreated {
        pane_id: PaneId,
    },
    Status {
        session: PersistedSession,
    },
    WorkspaceState {
        workspace_id: WorkspaceId,
        session: PersistedSession,
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
    pub response: Result<ControlResponse, String>,
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
