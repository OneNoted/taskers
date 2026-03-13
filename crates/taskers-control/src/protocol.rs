use serde::{Deserialize, Serialize};
use uuid::Uuid;

use taskers_domain::{
    AppModel, Direction, PaneId, PaneMetadataPatch, PersistedSession, SignalEvent, SplitAxis,
    WindowFrame, WindowId, WorkspaceId, WorkspaceViewport, WorkspaceWindowId,
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
    SetWorkspaceWindowFrame {
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        frame: WindowFrame,
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
