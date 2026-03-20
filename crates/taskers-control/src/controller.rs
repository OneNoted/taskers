use std::sync::{Arc, Mutex};

use taskers_domain::{AppModel, DomainError, WindowId, WorkspaceId};

use crate::protocol::{ControlCommand, ControlQuery, ControlResponse};

#[derive(Debug, Clone)]
pub struct InMemoryController {
    state: Arc<Mutex<ControllerState>>,
}

#[derive(Debug, Clone)]
struct ControllerState {
    model: AppModel,
    revision: u64,
}

#[derive(Debug, Clone)]
pub struct ControllerSnapshot {
    pub model: AppModel,
    pub revision: u64,
}

impl InMemoryController {
    pub fn new(state: AppModel) -> Self {
        Self {
            state: Arc::new(Mutex::new(ControllerState {
                model: state,
                revision: 0,
            })),
        }
    }

    pub fn snapshot(&self) -> ControllerSnapshot {
        let state = self.state.lock().expect("state mutex poisoned").clone();
        ControllerSnapshot {
            model: state.model,
            revision: state.revision,
        }
    }

    pub fn revision(&self) -> u64 {
        self.state.lock().expect("state mutex poisoned").revision
    }

    pub fn handle(&self, command: ControlCommand) -> Result<ControlResponse, DomainError> {
        let mut state = self.state.lock().expect("state mutex poisoned");
        let model = &mut state.model;

        let (response, mutated) = match command {
            ControlCommand::CreateWorkspace { label } => {
                let workspace_id = model.create_workspace(label);
                (ControlResponse::WorkspaceCreated { workspace_id }, true)
            }
            ControlCommand::RenameWorkspace {
                workspace_id,
                label,
            } => {
                model.rename_workspace(workspace_id, label)?;
                (
                    ControlResponse::Ack {
                        message: "workspace renamed".into(),
                    },
                    true,
                )
            }
            ControlCommand::SwitchWorkspace {
                window_id,
                workspace_id,
            } => {
                let target_window = window_id.unwrap_or(model.active_window);
                model.switch_workspace(target_window, workspace_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace switched".into(),
                    },
                    true,
                )
            }
            ControlCommand::SplitPane {
                workspace_id,
                pane_id,
                axis,
            } => {
                let new_pane_id = model.split_pane(workspace_id, pane_id, axis)?;
                (
                    ControlResponse::PaneSplit {
                        pane_id: new_pane_id,
                    },
                    true,
                )
            }
            ControlCommand::CreateWorkspaceWindow {
                workspace_id,
                direction,
            } => {
                let new_pane_id = model.create_workspace_window(workspace_id, direction)?;
                (
                    ControlResponse::WorkspaceWindowCreated {
                        pane_id: new_pane_id,
                    },
                    true,
                )
            }
            ControlCommand::FocusWorkspaceWindow {
                workspace_id,
                workspace_window_id,
            } => {
                model.focus_workspace_window(workspace_id, workspace_window_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace window focused".into(),
                    },
                    true,
                )
            }
            ControlCommand::FocusPane {
                workspace_id,
                pane_id,
            } => {
                model.focus_pane(workspace_id, pane_id)?;
                (
                    ControlResponse::Ack {
                        message: "pane focused".into(),
                    },
                    true,
                )
            }
            ControlCommand::FocusPaneDirection {
                workspace_id,
                direction,
            } => {
                model.focus_pane_direction(workspace_id, direction)?;
                (
                    ControlResponse::Ack {
                        message: "pane focus moved".into(),
                    },
                    true,
                )
            }
            ControlCommand::ResizeActiveWindow {
                workspace_id,
                direction,
                amount,
            } => {
                model.resize_active_window(workspace_id, direction, amount)?;
                (
                    ControlResponse::Ack {
                        message: "workspace window resized".into(),
                    },
                    true,
                )
            }
            ControlCommand::ResizeActivePaneSplit {
                workspace_id,
                direction,
                amount,
            } => {
                model.resize_active_pane_split(workspace_id, direction, amount)?;
                (
                    ControlResponse::Ack {
                        message: "pane split resized".into(),
                    },
                    true,
                )
            }
            ControlCommand::SetWorkspaceColumnWidth {
                workspace_id,
                workspace_column_id,
                width,
            } => {
                model.set_workspace_column_width(workspace_id, workspace_column_id, width)?;
                (
                    ControlResponse::Ack {
                        message: "workspace column width updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::SetWorkspaceWindowHeight {
                workspace_id,
                workspace_window_id,
                height,
            } => {
                model.set_workspace_window_height(workspace_id, workspace_window_id, height)?;
                (
                    ControlResponse::Ack {
                        message: "workspace window height updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::SetWindowSplitRatio {
                workspace_id,
                workspace_window_id,
                path,
                ratio,
            } => {
                model.set_window_split_ratio(workspace_id, workspace_window_id, &path, ratio)?;
                (
                    ControlResponse::Ack {
                        message: "window split ratio updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::UpdatePaneMetadata { pane_id, patch } => {
                model.update_pane_metadata(pane_id, patch)?;
                (
                    ControlResponse::Ack {
                        message: "pane metadata updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::UpdateSurfaceMetadata { surface_id, patch } => {
                model.update_surface_metadata(surface_id, patch)?;
                (
                    ControlResponse::Ack {
                        message: "surface metadata updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::CreateSurface {
                workspace_id,
                pane_id,
                kind,
            } => {
                let surface_id = model.create_surface(workspace_id, pane_id, kind)?;
                (ControlResponse::SurfaceCreated { surface_id }, true)
            }
            ControlCommand::FocusSurface {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.focus_surface(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "surface focused".into(),
                    },
                    true,
                )
            }
            ControlCommand::MarkSurfaceCompleted {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.mark_surface_completed(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "surface marked completed".into(),
                    },
                    true,
                )
            }
            ControlCommand::CloseSurface {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.close_surface(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "surface closed".into(),
                    },
                    true,
                )
            }
            ControlCommand::MoveSurface {
                workspace_id,
                pane_id,
                surface_id,
                to_index,
            } => {
                model.move_surface(workspace_id, pane_id, surface_id, to_index)?;
                (
                    ControlResponse::Ack {
                        message: "surface moved".into(),
                    },
                    true,
                )
            }
            ControlCommand::TransferSurface {
                workspace_id,
                source_pane_id,
                surface_id,
                target_pane_id,
                to_index,
            } => {
                model.transfer_surface(
                    workspace_id,
                    source_pane_id,
                    surface_id,
                    target_pane_id,
                    to_index,
                )?;
                (
                    ControlResponse::Ack {
                        message: "surface transferred".into(),
                    },
                    true,
                )
            }
            ControlCommand::SetWorkspaceViewport {
                workspace_id,
                viewport,
            } => {
                model.set_workspace_viewport(workspace_id, viewport)?;
                (
                    ControlResponse::Ack {
                        message: "workspace viewport updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::ClosePane {
                workspace_id,
                pane_id,
            } => {
                model.close_pane(workspace_id, pane_id)?;
                (
                    ControlResponse::Ack {
                        message: "pane closed".into(),
                    },
                    true,
                )
            }
            ControlCommand::CloseWorkspace { workspace_id } => {
                model.close_workspace(workspace_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace closed".into(),
                    },
                    true,
                )
            }
            ControlCommand::ReorderWorkspaces {
                window_id,
                workspace_ids,
            } => {
                model.reorder_workspaces(window_id, workspace_ids)?;
                (
                    ControlResponse::Ack {
                        message: "workspaces reordered".into(),
                    },
                    true,
                )
            }
            ControlCommand::EmitSignal {
                workspace_id,
                pane_id,
                surface_id,
                event,
            } => {
                if let Some(surface_id) = surface_id {
                    model.apply_surface_signal(workspace_id, pane_id, surface_id, event)?;
                } else {
                    model.apply_signal(workspace_id, pane_id, event)?;
                }
                (
                    ControlResponse::Ack {
                        message: "signal applied".into(),
                    },
                    true,
                )
            }
            ControlCommand::QueryStatus { query } => match query {
                ControlQuery::ActiveWindow | ControlQuery::All => (
                    ControlResponse::Status {
                        session: model.snapshot(),
                    },
                    false,
                ),
                ControlQuery::Window { window_id } => (window_snapshot(model, window_id)?, false),
                ControlQuery::Workspace { workspace_id } => {
                    (workspace_snapshot(model, workspace_id)?, false)
                }
            },
        };

        if mutated {
            state.revision = state.revision.saturating_add(1);
        }

        Ok(response)
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

#[cfg(test)]
mod tests {
    use taskers_domain::{AppModel, SignalEvent, SignalKind};

    use crate::{ControlCommand, ControlQuery};

    use super::InMemoryController;

    #[test]
    fn revision_increments_for_mutations_but_not_queries() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        assert_eq!(controller.revision(), 0);

        controller
            .handle(ControlCommand::QueryStatus {
                query: ControlQuery::All,
            })
            .expect("query status");
        assert_eq!(controller.revision(), 0);

        controller
            .handle(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .expect("create workspace");
        assert_eq!(controller.revision(), 1);
        assert_eq!(controller.snapshot().revision, 1);
    }

    #[test]
    fn revision_increments_for_signal_mutations() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");

        controller
            .handle(ControlCommand::EmitSignal {
                workspace_id: workspace.id,
                pane_id: workspace.active_pane,
                surface_id: None,
                event: SignalEvent::new("pty", SignalKind::Progress, Some("Running".into())),
            })
            .expect("emit signal");

        assert_eq!(controller.revision(), 1);
    }
}
