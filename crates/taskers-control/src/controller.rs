use std::sync::{Arc, Mutex};

use taskers_domain::{AppModel, DomainError, WindowId, WorkspaceId};

use crate::protocol::{ControlCommand, ControlQuery, ControlResponse};

#[derive(Debug, Clone)]
pub struct InMemoryController {
    state: Arc<Mutex<AppModel>>,
}

#[derive(Debug, Clone)]
pub struct ControllerSnapshot {
    pub model: AppModel,
}

impl InMemoryController {
    pub fn new(state: AppModel) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn snapshot(&self) -> ControllerSnapshot {
        let state = self.state.lock().expect("state mutex poisoned").clone();
        ControllerSnapshot { model: state }
    }

    pub fn handle(&self, command: ControlCommand) -> Result<ControlResponse, DomainError> {
        let mut model = self.state.lock().expect("state mutex poisoned");

        match command {
            ControlCommand::CreateWorkspace { label } => {
                let workspace_id = model.create_workspace(label);
                Ok(ControlResponse::WorkspaceCreated { workspace_id })
            }
            ControlCommand::RenameWorkspace {
                workspace_id,
                label,
            } => {
                model.rename_workspace(workspace_id, label)?;
                Ok(ControlResponse::Ack {
                    message: "workspace renamed".into(),
                })
            }
            ControlCommand::SwitchWorkspace {
                window_id,
                workspace_id,
            } => {
                let target_window = window_id.unwrap_or(model.active_window);
                model.switch_workspace(target_window, workspace_id)?;
                Ok(ControlResponse::Ack {
                    message: "workspace switched".into(),
                })
            }
            ControlCommand::SplitPane {
                workspace_id,
                pane_id,
                axis,
            } => {
                let new_pane_id = model.split_pane(workspace_id, pane_id, axis)?;
                Ok(ControlResponse::PaneSplit {
                    pane_id: new_pane_id,
                })
            }
            ControlCommand::CreateWorkspaceWindow {
                workspace_id,
                direction,
            } => {
                let new_pane_id = model.create_workspace_window(workspace_id, direction)?;
                Ok(ControlResponse::WorkspaceWindowCreated {
                    pane_id: new_pane_id,
                })
            }
            ControlCommand::FocusWorkspaceWindow {
                workspace_id,
                workspace_window_id,
            } => {
                model.focus_workspace_window(workspace_id, workspace_window_id)?;
                Ok(ControlResponse::Ack {
                    message: "workspace window focused".into(),
                })
            }
            ControlCommand::FocusPane {
                workspace_id,
                pane_id,
            } => {
                model.focus_pane(workspace_id, pane_id)?;
                Ok(ControlResponse::Ack {
                    message: "pane focused".into(),
                })
            }
            ControlCommand::FocusPaneDirection {
                workspace_id,
                direction,
            } => {
                model.focus_pane_direction(workspace_id, direction)?;
                Ok(ControlResponse::Ack {
                    message: "pane focus moved".into(),
                })
            }
            ControlCommand::ResizeActiveWindow {
                workspace_id,
                direction,
                amount,
            } => {
                model.resize_active_window(workspace_id, direction, amount)?;
                Ok(ControlResponse::Ack {
                    message: "workspace window resized".into(),
                })
            }
            ControlCommand::ResizeActivePaneSplit {
                workspace_id,
                direction,
                amount,
            } => {
                model.resize_active_pane_split(workspace_id, direction, amount)?;
                Ok(ControlResponse::Ack {
                    message: "pane split resized".into(),
                })
            }
            ControlCommand::SetWorkspaceWindowFrame {
                workspace_id,
                workspace_window_id,
                frame,
            } => {
                model.set_workspace_window_frame(workspace_id, workspace_window_id, frame)?;
                Ok(ControlResponse::Ack {
                    message: "workspace window frame updated".into(),
                })
            }
            ControlCommand::SetWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path,
                ratio,
            } => {
                model.set_window_split_ratio(workspace_id, workspace_window_id, &path, ratio)?;
                Ok(ControlResponse::Ack {
                    message: "window split ratio updated".into(),
                })
            }
            ControlCommand::UpdatePaneMetadata { pane_id, patch } => {
                model.update_pane_metadata(pane_id, patch)?;
                Ok(ControlResponse::Ack {
                    message: "pane metadata updated".into(),
                })
            }
            ControlCommand::SetWorkspaceViewport {
                workspace_id,
                viewport,
            } => {
                model.set_workspace_viewport(workspace_id, viewport)?;
                Ok(ControlResponse::Ack {
                    message: "workspace viewport updated".into(),
                })
            }
            ControlCommand::ClosePane {
                workspace_id,
                pane_id,
            } => {
                model.close_pane(workspace_id, pane_id)?;
                Ok(ControlResponse::Ack {
                    message: "pane closed".into(),
                })
            }
            ControlCommand::CloseWorkspace { workspace_id } => {
                model.close_workspace(workspace_id)?;
                Ok(ControlResponse::Ack {
                    message: "workspace closed".into(),
                })
            }
            ControlCommand::EmitSignal {
                workspace_id,
                pane_id,
                event,
            } => {
                model.apply_signal(workspace_id, pane_id, event)?;
                Ok(ControlResponse::Ack {
                    message: "signal applied".into(),
                })
            }
            ControlCommand::QueryStatus { query } => match query {
                ControlQuery::ActiveWindow | ControlQuery::All => Ok(ControlResponse::Status {
                    session: model.snapshot(),
                }),
                ControlQuery::Window { window_id } => window_snapshot(&model, window_id),
                ControlQuery::Workspace { workspace_id } => {
                    workspace_snapshot(&model, workspace_id)
                }
            },
        }
    }
}

fn window_snapshot(model: &AppModel, window_id: WindowId) -> Result<ControlResponse, DomainError> {
    let _ = model
        .windows
        .get(&window_id)
        .ok_or(DomainError::MissingWindow(window_id))?;
    Ok(ControlResponse::Status {
        session: model.snapshot(),
    })
}

fn workspace_snapshot(
    model: &AppModel,
    workspace_id: WorkspaceId,
) -> Result<ControlResponse, DomainError> {
    let _ = model
        .workspaces
        .get(&workspace_id)
        .ok_or(DomainError::MissingWorkspace(workspace_id))?;
    Ok(ControlResponse::WorkspaceState {
        workspace_id,
        session: model.snapshot(),
    })
}
