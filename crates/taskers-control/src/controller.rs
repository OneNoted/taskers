use std::sync::{Arc, Mutex};

use taskers_domain::{
    AppModel, BrowserProfileMode, Direction, DomainError, PaneId, PaneKind, SurfaceId,
    SurfaceRecord, WindowId, WorkspaceId,
};

use crate::protocol::{
    ControlCommand, ControlQuery, ControlResponse, IdentifyContext, IdentifyResult,
};

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
            ControlCommand::SplitPaneDirection {
                workspace_id,
                pane_id,
                direction,
            } => {
                let new_pane_id =
                    model.split_pane_direction(workspace_id, Some(pane_id), direction)?;
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
                preferred_column_width,
                preferred_window_height,
            } => {
                let new_pane_id = model.create_workspace_window(workspace_id, direction)?;
                match direction {
                    Direction::Left | Direction::Right => {
                        if let Some(width) = preferred_column_width
                            && let Some(workspace_column_id) = model
                                .workspaces
                                .get(&workspace_id)
                                .and_then(|workspace| workspace.active_column_id())
                        {
                            model.set_workspace_column_width(
                                workspace_id,
                                workspace_column_id,
                                width,
                            )?;
                        }
                    }
                    Direction::Up | Direction::Down => {
                        if let Some(height) = preferred_window_height
                            && let Some(workspace_window_id) = model
                                .workspaces
                                .get(&workspace_id)
                                .map(|workspace| workspace.active_window)
                        {
                            model.set_workspace_window_height(
                                workspace_id,
                                workspace_window_id,
                                height,
                            )?;
                        }
                    }
                }
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
            ControlCommand::MoveWorkspaceWindow {
                workspace_id,
                workspace_window_id,
                target,
            } => {
                model.move_workspace_window(workspace_id, workspace_window_id, target)?;
                (
                    ControlResponse::Ack {
                        message: "workspace window moved".into(),
                    },
                    true,
                )
            }
            ControlCommand::CreateWorkspaceWindowTab {
                workspace_id,
                workspace_window_id,
            } => {
                let (workspace_window_tab_id, pane_id) =
                    model.create_workspace_window_tab(workspace_id, workspace_window_id)?;
                (
                    ControlResponse::WorkspaceWindowTabCreated {
                        pane_id,
                        workspace_window_tab_id,
                    },
                    true,
                )
            }
            ControlCommand::FocusWorkspaceWindowTab {
                workspace_id,
                workspace_window_id,
                workspace_window_tab_id,
            } => {
                model.focus_workspace_window_tab(
                    workspace_id,
                    workspace_window_id,
                    workspace_window_tab_id,
                )?;
                (
                    ControlResponse::Ack {
                        message: "workspace window tab focused".into(),
                    },
                    true,
                )
            }
            ControlCommand::MoveWorkspaceWindowTab {
                workspace_id,
                workspace_window_id,
                workspace_window_tab_id,
                to_index,
            } => {
                model.move_workspace_window_tab(
                    workspace_id,
                    workspace_window_id,
                    workspace_window_tab_id,
                    to_index,
                )?;
                (
                    ControlResponse::Ack {
                        message: "workspace window tab moved".into(),
                    },
                    true,
                )
            }
            ControlCommand::TransferWorkspaceWindowTab {
                workspace_id,
                source_workspace_window_id,
                workspace_window_tab_id,
                target_workspace_window_id,
                to_index,
            } => {
                model.transfer_workspace_window_tab(
                    workspace_id,
                    source_workspace_window_id,
                    workspace_window_tab_id,
                    target_workspace_window_id,
                    to_index,
                )?;
                (
                    ControlResponse::Ack {
                        message: "workspace window tab transferred".into(),
                    },
                    true,
                )
            }
            ControlCommand::ExtractWorkspaceWindowTab {
                workspace_id,
                source_workspace_window_id,
                workspace_window_tab_id,
                target,
            } => {
                model.extract_workspace_window_tab(
                    workspace_id,
                    source_workspace_window_id,
                    workspace_window_tab_id,
                    target,
                )?;
                (
                    ControlResponse::Ack {
                        message: "workspace window tab extracted".into(),
                    },
                    true,
                )
            }
            ControlCommand::CloseWorkspaceWindowTab {
                workspace_id,
                workspace_window_id,
                workspace_window_tab_id,
            } => {
                model.close_workspace_window_tab(
                    workspace_id,
                    workspace_window_id,
                    workspace_window_tab_id,
                )?;
                (
                    ControlResponse::Ack {
                        message: "workspace window tab closed".into(),
                    },
                    true,
                )
            }
            ControlCommand::CreatePaneTab {
                workspace_id,
                pane_container_id,
                kind,
            } => {
                let (pane_tab_id, pane_id) =
                    model.create_pane_tab(workspace_id, pane_container_id, kind)?;
                (
                    ControlResponse::PaneTabCreated {
                        pane_id,
                        pane_tab_id,
                    },
                    true,
                )
            }
            ControlCommand::FocusPaneTab {
                workspace_id,
                pane_container_id,
                pane_tab_id,
            } => {
                model.focus_pane_tab(workspace_id, pane_container_id, pane_tab_id)?;
                (
                    ControlResponse::Ack {
                        message: "pane tab focused".into(),
                    },
                    true,
                )
            }
            ControlCommand::MovePaneTab {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                to_index,
            } => {
                model.move_pane_tab(workspace_id, pane_container_id, pane_tab_id, to_index)?;
                (
                    ControlResponse::Ack {
                        message: "pane tab moved".into(),
                    },
                    true,
                )
            }
            ControlCommand::TransferPaneTab {
                workspace_id,
                source_pane_container_id,
                pane_tab_id,
                target_pane_container_id,
                to_index,
            } => {
                model.transfer_pane_tab(
                    workspace_id,
                    source_pane_container_id,
                    pane_tab_id,
                    target_pane_container_id,
                    to_index,
                )?;
                (
                    ControlResponse::Ack {
                        message: "pane tab transferred".into(),
                    },
                    true,
                )
            }
            ControlCommand::ClosePaneTab {
                workspace_id,
                pane_container_id,
                pane_tab_id,
            } => {
                model.close_pane_tab(workspace_id, pane_container_id, pane_tab_id)?;
                (
                    ControlResponse::Ack {
                        message: "pane tab closed".into(),
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
            ControlCommand::SetPaneTabSplitRatio {
                workspace_id,
                pane_container_id,
                pane_tab_id,
                path,
                ratio,
            } => {
                model.set_pane_tab_split_ratio(
                    workspace_id,
                    pane_container_id,
                    pane_tab_id,
                    &path,
                    ratio,
                )?;
                (
                    ControlResponse::Ack {
                        message: "pane tab split ratio updated".into(),
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
                browser_profile_mode,
            } => {
                let is_browser = matches!(kind, PaneKind::Browser);
                let surface_id = model.create_surface(workspace_id, pane_id, kind)?;
                if is_browser {
                    model.update_surface_metadata(
                        surface_id,
                        taskers_domain::PaneMetadataPatch {
                            browser_profile_mode: Some(
                                browser_profile_mode
                                    .unwrap_or(BrowserProfileMode::PersistentDefault),
                            ),
                            ..taskers_domain::PaneMetadataPatch::default()
                        },
                    )?;
                }
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
            ControlCommand::StartSurfaceAgentSession {
                workspace_id: _,
                pane_id: _,
                surface_id,
                agent_kind,
            } => {
                let current = resolve_identify_context(model, None, None, Some(surface_id))?;
                model.start_surface_agent_session(
                    current.workspace_id,
                    current.pane_id,
                    surface_id,
                    agent_kind,
                )?;
                (
                    ControlResponse::Ack {
                        message: "surface agent session started".into(),
                    },
                    true,
                )
            }
            ControlCommand::StopSurfaceAgentSession {
                workspace_id: _,
                pane_id: _,
                surface_id,
                exit_status,
            } => {
                let current = resolve_identify_context(model, None, None, Some(surface_id))?;
                model.stop_surface_agent_session(
                    current.workspace_id,
                    current.pane_id,
                    surface_id,
                    exit_status,
                )?;
                (
                    ControlResponse::Ack {
                        message: "surface agent session stopped".into(),
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
                source_workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
                target_pane_id,
                to_index,
            } => {
                model.transfer_surface(
                    source_workspace_id,
                    source_pane_id,
                    surface_id,
                    target_workspace_id,
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
            ControlCommand::MoveSurfaceToSplit {
                source_workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
                target_pane_id,
                direction,
            } => {
                let new_pane_id = model.move_surface_to_split(
                    source_workspace_id,
                    source_pane_id,
                    surface_id,
                    target_workspace_id,
                    target_pane_id,
                    direction,
                )?;
                (
                    ControlResponse::SurfaceMovedToSplit {
                        pane_id: new_pane_id,
                    },
                    true,
                )
            }
            ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id,
                target_workspace_id,
            } => {
                let new_pane_id = model.move_surface_to_workspace(
                    source_workspace_id,
                    source_pane_id,
                    surface_id,
                    target_workspace_id,
                )?;
                (
                    ControlResponse::SurfaceMovedToWorkspace {
                        pane_id: new_pane_id,
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
                    let current = resolve_identify_context(model, None, None, Some(surface_id))?;
                    model.apply_surface_signal(
                        current.workspace_id,
                        current.pane_id,
                        surface_id,
                        event,
                    )?;
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
            ControlCommand::AgentSetStatus { workspace_id, text } => {
                model.set_workspace_status(workspace_id, text)?;
                (
                    ControlResponse::Ack {
                        message: "workspace agent status updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentClearStatus { workspace_id } => {
                model.clear_workspace_status(workspace_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace agent status cleared".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentSetProgress {
                workspace_id,
                progress,
            } => {
                model.set_workspace_progress(workspace_id, progress)?;
                (
                    ControlResponse::Ack {
                        message: "workspace progress updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentClearProgress { workspace_id } => {
                model.clear_workspace_progress(workspace_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace progress cleared".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentAppendLog {
                workspace_id,
                entry,
            } => {
                model.append_workspace_log(workspace_id, entry)?;
                (
                    ControlResponse::Ack {
                        message: "workspace log appended".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentClearLog { workspace_id } => {
                model.clear_workspace_log(workspace_id)?;
                (
                    ControlResponse::Ack {
                        message: "workspace log cleared".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentCreateNotification {
                target,
                kind,
                title,
                subtitle,
                external_id,
                message,
                state,
            } => {
                model.create_agent_notification(
                    target,
                    kind,
                    title,
                    subtitle,
                    external_id,
                    message,
                    state,
                )?;
                (
                    ControlResponse::Ack {
                        message: "agent notification created".into(),
                    },
                    true,
                )
            }
            ControlCommand::OpenNotification {
                window_id,
                notification_id,
            } => {
                model
                    .open_notification(window_id.unwrap_or(model.active_window), notification_id)?;
                (
                    ControlResponse::Ack {
                        message: "notification opened".into(),
                    },
                    true,
                )
            }
            ControlCommand::ClearNotification { notification_id } => {
                model.clear_notification(notification_id)?;
                (
                    ControlResponse::Ack {
                        message: "notification cleared".into(),
                    },
                    true,
                )
            }
            ControlCommand::MarkNotificationDelivery {
                notification_id,
                delivery,
            } => {
                model.mark_notification_delivery(notification_id, delivery)?;
                (
                    ControlResponse::Ack {
                        message: "notification delivery updated".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentClearNotifications { target } => {
                model.clear_agent_notifications(target)?;
                (
                    ControlResponse::Ack {
                        message: "agent notifications cleared".into(),
                    },
                    true,
                )
            }
            ControlCommand::DismissSurfaceAlert {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.dismiss_surface_alert(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "surface alert dismissed".into(),
                    },
                    true,
                )
            }
            ControlCommand::DismissInterruptedAgentResume {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.dismiss_interrupted_agent_resume(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "interrupted agent resume dismissed".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentTriggerFlash {
                workspace_id,
                pane_id,
                surface_id,
            } => {
                model.trigger_surface_flash(workspace_id, pane_id, surface_id)?;
                (
                    ControlResponse::Ack {
                        message: "surface flash triggered".into(),
                    },
                    true,
                )
            }
            ControlCommand::AgentFocusLatestUnread { window_id } => {
                model.focus_latest_unread(window_id.unwrap_or(model.active_window))?;
                (
                    ControlResponse::Ack {
                        message: "focused latest unread activity".into(),
                    },
                    true,
                )
            }
            ControlCommand::Browser { .. } => {
                return Err(DomainError::InvalidOperation(
                    "browser automation commands require a live GTK host",
                ));
            }
            ControlCommand::Screenshot { .. } => {
                return Err(DomainError::InvalidOperation(
                    "screenshot commands require a live GTK host",
                ));
            }
            ControlCommand::TerminalDebug { .. } => {
                return Err(DomainError::InvalidOperation(
                    "terminal debug commands require a live GTK host",
                ));
            }
            ControlCommand::Vcs { .. } => {
                return Err(DomainError::InvalidOperation(
                    "vcs commands require app runtime support",
                ));
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
                ControlQuery::Identify {
                    workspace_id,
                    pane_id,
                    surface_id,
                } => (
                    ControlResponse::Identify {
                        result: identify_snapshot(model, workspace_id, pane_id, surface_id)?,
                    },
                    false,
                ),
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

fn identify_snapshot(
    model: &AppModel,
    workspace_id: Option<WorkspaceId>,
    pane_id: Option<PaneId>,
    surface_id: Option<SurfaceId>,
) -> Result<IdentifyResult, DomainError> {
    let focused = focused_identify_context(model)?;
    let caller = if workspace_id.is_some() || pane_id.is_some() || surface_id.is_some() {
        Some(resolve_identify_context(
            model,
            workspace_id,
            pane_id,
            surface_id,
        )?)
    } else {
        None
    };

    Ok(IdentifyResult { focused, caller })
}

fn focused_identify_context(model: &AppModel) -> Result<IdentifyContext, DomainError> {
    let window_id = model.active_window;
    let workspace = model
        .active_workspace()
        .ok_or(DomainError::InvalidOperation("app has no active workspace"))?;
    let pane = workspace
        .panes
        .get(&workspace.active_pane)
        .ok_or(DomainError::MissingPane(workspace.active_pane))?;
    let surface = pane
        .surfaces
        .get(&pane.active_surface)
        .ok_or(DomainError::MissingSurface(pane.active_surface))?;

    Ok(identify_context_from_parts(
        window_id, workspace, pane.id, surface,
    ))
}

fn resolve_identify_context(
    model: &AppModel,
    workspace_id: Option<WorkspaceId>,
    pane_id: Option<PaneId>,
    surface_id: Option<SurfaceId>,
) -> Result<IdentifyContext, DomainError> {
    let window_id = model.active_window;

    if let Some(surface_id) = surface_id {
        for (candidate_workspace_id, workspace) in &model.workspaces {
            if workspace_id.is_some_and(|expected| expected != *candidate_workspace_id) {
                continue;
            }
            for (candidate_pane_id, pane) in &workspace.panes {
                if pane_id.is_some_and(|expected| expected != *candidate_pane_id) {
                    continue;
                }
                if let Some(surface) = pane.surfaces.get(&surface_id) {
                    return Ok(identify_context_from_parts(
                        window_id,
                        workspace,
                        *candidate_pane_id,
                        surface,
                    ));
                }
            }
        }
        return Err(DomainError::MissingSurface(surface_id));
    }

    if let Some(pane_id) = pane_id {
        for (candidate_workspace_id, workspace) in &model.workspaces {
            if workspace_id.is_some_and(|expected| expected != *candidate_workspace_id) {
                continue;
            }
            if let Some(pane) = workspace.panes.get(&pane_id) {
                let surface = pane
                    .surfaces
                    .get(&pane.active_surface)
                    .ok_or(DomainError::MissingSurface(pane.active_surface))?;
                return Ok(identify_context_from_parts(
                    window_id, workspace, pane_id, surface,
                ));
            }
        }
        return Err(DomainError::MissingPane(pane_id));
    }

    if let Some(workspace_id) = workspace_id {
        let workspace = model
            .workspaces
            .get(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let pane = workspace
            .panes
            .get(&workspace.active_pane)
            .ok_or(DomainError::MissingPane(workspace.active_pane))?;
        let surface = pane
            .surfaces
            .get(&pane.active_surface)
            .ok_or(DomainError::MissingSurface(pane.active_surface))?;
        return Ok(identify_context_from_parts(
            window_id, workspace, pane.id, surface,
        ));
    }

    focused_identify_context(model)
}

fn identify_context_from_parts(
    window_id: WindowId,
    workspace: &taskers_domain::Workspace,
    pane_id: PaneId,
    surface: &SurfaceRecord,
) -> IdentifyContext {
    IdentifyContext {
        window_id,
        workspace_id: workspace.id,
        workspace_label: workspace.label.clone(),
        workspace_window_id: workspace.window_for_pane(pane_id),
        pane_id,
        surface_id: surface.id,
        surface_kind: surface.kind.clone(),
        title: normalized_value(surface.metadata.title.as_deref()),
        cwd: normalized_value(surface.metadata.cwd.as_deref()),
        url: normalized_value(surface.metadata.url.as_deref()),
        loading: matches!(surface.kind, PaneKind::Browser).then_some(false),
    }
}

fn normalized_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use taskers_domain::{AppModel, BrowserProfileMode, PaneKind, SignalEvent, SignalKind};

    use crate::{
        ControlCommand, ControlQuery, ControlResponse, ScreenshotCommand, ScreenshotTarget,
    };

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

    #[test]
    fn create_workspace_window_applies_preferred_column_width_when_requested() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace_id = snapshot.model.active_workspace_id().expect("workspace");

        controller
            .handle(ControlCommand::CreateWorkspaceWindow {
                workspace_id,
                direction: taskers_domain::Direction::Right,
                preferred_column_width: Some(taskers_domain::MIN_WORKSPACE_WINDOW_WIDTH),
                preferred_window_height: None,
            })
            .expect("create workspace window");

        let snapshot = controller.snapshot();
        let workspace = snapshot
            .model
            .workspaces
            .get(&workspace_id)
            .expect("workspace");
        let active_column_id = workspace.active_column_id().expect("active column");
        let active_column = workspace
            .columns
            .get(&active_column_id)
            .expect("active column record");

        assert_eq!(
            active_column.width,
            taskers_domain::MIN_WORKSPACE_WINDOW_WIDTH
        );
    }

    #[test]
    fn surface_signals_follow_a_moved_surface_even_with_stale_pane_context() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let source_workspace = snapshot.model.active_workspace().expect("workspace");
        let source_workspace_id = source_workspace.id;
        let source_pane_id = source_workspace.active_pane;

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::PersistentDefault),
            })
            .expect("create moved surface");
        let moved_surface_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&source_workspace_id)
            .and_then(|workspace| workspace.panes.get(&source_pane_id))
            .map(|pane| pane.active_surface)
            .expect("moved surface");

        controller
            .handle(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .expect("create target workspace");
        let target_workspace_id = controller
            .snapshot()
            .model
            .active_workspace_id()
            .expect("target workspace");

        controller
            .handle(ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id: moved_surface_id,
                target_workspace_id,
            })
            .expect("move surface");

        controller
            .handle(ControlCommand::EmitSignal {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                surface_id: Some(moved_surface_id),
                event: SignalEvent::new("pty", SignalKind::Progress, Some("Running".into())),
            })
            .expect("emit moved surface signal");

        let snapshot = controller.snapshot();
        let target_surface = snapshot
            .model
            .workspaces
            .values()
            .flat_map(|workspace| {
                workspace.panes.values().flat_map(move |pane| {
                    pane.surfaces
                        .values()
                        .map(move |surface| (workspace, pane, surface))
                })
            })
            .find(|(_, _, surface)| surface.id == moved_surface_id)
            .expect("target surface");

        assert_eq!(target_surface.0.id, target_workspace_id);
        assert_eq!(
            target_surface.2.attention,
            taskers_domain::AttentionState::Busy
        );
        assert_eq!(
            snapshot
                .model
                .workspaces
                .get(&source_workspace_id)
                .and_then(|workspace| workspace.panes.get(&source_pane_id))
                .and_then(|pane| pane.active_surface())
                .map(|surface| surface.attention),
            Some(taskers_domain::AttentionState::Normal)
        );
    }

    #[test]
    fn surface_agent_start_follows_a_moved_surface_even_with_stale_pane_context() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let source_workspace = snapshot.model.active_workspace().expect("workspace");
        let source_workspace_id = source_workspace.id;
        let source_pane_id = source_workspace.active_pane;

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::PersistentDefault),
            })
            .expect("create moved surface");
        let moved_surface_id = controller
            .snapshot()
            .model
            .workspaces
            .get(&source_workspace_id)
            .and_then(|workspace| workspace.panes.get(&source_pane_id))
            .map(|pane| pane.active_surface)
            .expect("moved surface");

        controller
            .handle(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .expect("create target workspace");
        let target_workspace_id = controller
            .snapshot()
            .model
            .active_workspace_id()
            .expect("target workspace");

        controller
            .handle(ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id: moved_surface_id,
                target_workspace_id,
            })
            .expect("move surface");

        controller
            .handle(ControlCommand::StartSurfaceAgentSession {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                surface_id: moved_surface_id,
                agent_kind: "codex".into(),
            })
            .expect("start moved surface agent session");

        let snapshot = controller.snapshot();
        let target_surface = snapshot
            .model
            .workspaces
            .values()
            .flat_map(|workspace| {
                workspace.panes.values().flat_map(move |pane| {
                    pane.surfaces
                        .values()
                        .map(move |surface| (workspace, pane, surface))
                })
            })
            .find(|(_, _, surface)| surface.id == moved_surface_id)
            .expect("target surface");

        assert_eq!(target_surface.0.id, target_workspace_id);
        assert!(target_surface.2.agent_process.is_some());
        assert!(target_surface.2.agent_session.is_none());
        assert_eq!(
            target_surface.2.metadata.agent_kind.as_deref(),
            Some("codex")
        );
        assert!(target_surface.2.metadata.agent_active);
        assert!(
            snapshot
                .model
                .workspaces
                .get(&source_workspace_id)
                .and_then(|workspace| workspace.panes.get(&source_pane_id))
                .and_then(|pane| pane.active_surface())
                .and_then(|surface| surface.agent_process.as_ref())
                .is_none()
        );
    }

    #[test]
    fn surface_agent_stop_follows_a_moved_surface_even_with_stale_pane_context() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let source_workspace = snapshot.model.active_workspace().expect("workspace");
        let source_workspace_id = source_workspace.id;
        let source_pane_id = source_workspace.active_pane;

        let moved_surface_id = source_workspace
            .panes
            .get(&source_pane_id)
            .and_then(|pane| pane.active_surface())
            .map(|surface| surface.id)
            .expect("surface");

        controller
            .handle(ControlCommand::StartSurfaceAgentSession {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                surface_id: moved_surface_id,
                agent_kind: "codex".into(),
            })
            .expect("start surface agent session");

        controller
            .handle(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .expect("create target workspace");
        let target_workspace_id = controller
            .snapshot()
            .model
            .active_workspace_id()
            .expect("target workspace");

        controller
            .handle(ControlCommand::MoveSurfaceToWorkspace {
                source_workspace_id,
                source_pane_id,
                surface_id: moved_surface_id,
                target_workspace_id,
            })
            .expect("move surface");

        controller
            .handle(ControlCommand::StopSurfaceAgentSession {
                workspace_id: source_workspace_id,
                pane_id: source_pane_id,
                surface_id: moved_surface_id,
                exit_status: 1,
            })
            .expect("stop moved surface agent session");

        let snapshot = controller.snapshot();
        let target_surface = snapshot
            .model
            .workspaces
            .values()
            .flat_map(|workspace| {
                workspace.panes.values().flat_map(move |pane| {
                    pane.surfaces
                        .values()
                        .map(move |surface| (workspace, pane, surface))
                })
            })
            .find(|(_, _, surface)| surface.id == moved_surface_id)
            .expect("target surface");

        assert_eq!(target_surface.0.id, target_workspace_id);
        assert!(target_surface.2.agent_process.is_none());
        assert!(target_surface.2.agent_session.is_none());
        assert_eq!(
            target_surface.2.attention,
            taskers_domain::AttentionState::Error
        );
        assert!(
            snapshot
                .model
                .workspaces
                .get(&source_workspace_id)
                .and_then(|workspace| workspace.panes.get(&source_pane_id))
                .and_then(|pane| pane.active_surface())
                .and_then(|surface| surface.agent_process.as_ref())
                .is_none()
        );
    }

    #[test]
    fn identify_returns_focused_context_and_optional_caller() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");
        let pane = workspace
            .panes
            .get(&workspace.active_pane)
            .expect("active pane");
        let surface = pane.active_surface().expect("active surface");

        let response = controller
            .handle(ControlCommand::QueryStatus {
                query: ControlQuery::Identify {
                    workspace_id: None,
                    pane_id: None,
                    surface_id: None,
                },
            })
            .expect("identify focused");
        let ControlResponse::Identify { result } = response else {
            panic!("unexpected identify response");
        };
        assert_eq!(result.focused.workspace_id, workspace.id);
        assert_eq!(result.focused.pane_id, workspace.active_pane);
        assert_eq!(result.focused.surface_id, surface.id);
        assert_eq!(result.focused.surface_kind, PaneKind::Terminal);
        assert!(result.caller.is_none());

        let response = controller
            .handle(ControlCommand::QueryStatus {
                query: ControlQuery::Identify {
                    workspace_id: Some(workspace.id),
                    pane_id: Some(workspace.active_pane),
                    surface_id: Some(surface.id),
                },
            })
            .expect("identify caller");
        let ControlResponse::Identify { result } = response else {
            panic!("unexpected identify response");
        };
        let caller = result.caller.expect("caller context");
        assert_eq!(caller.workspace_id, workspace.id);
        assert_eq!(caller.pane_id, workspace.active_pane);
        assert_eq!(caller.surface_id, surface.id);
    }

    #[test]
    fn create_surface_applies_requested_browser_profile_mode() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");

        controller
            .handle(ControlCommand::CreateSurface {
                workspace_id: workspace.id,
                pane_id: workspace.active_pane,
                kind: PaneKind::Browser,
                browser_profile_mode: Some(BrowserProfileMode::Ephemeral),
            })
            .expect("create browser surface");

        let snapshot = controller.snapshot();
        let browser_surface = snapshot
            .model
            .workspaces
            .get(&workspace.id)
            .and_then(|workspace| workspace.panes.get(&workspace.active_pane))
            .and_then(|pane| pane.active_surface())
            .expect("browser surface");

        assert_eq!(
            browser_surface.metadata.browser_profile_mode,
            BrowserProfileMode::Ephemeral
        );
    }

    #[test]
    fn screenshot_commands_require_live_host() {
        let controller = InMemoryController::new(AppModel::new("Main"));
        let snapshot = controller.snapshot();
        let workspace = snapshot.model.active_workspace().expect("workspace");

        let error = controller
            .handle(ControlCommand::Screenshot {
                screenshot_command: ScreenshotCommand::Capture {
                    target: ScreenshotTarget::WorkspaceCanvas {
                        workspace_id: workspace.id,
                    },
                    path: None,
                },
            })
            .expect_err("screenshot should require live host");

        assert!(
            error.to_string().contains("live GTK host"),
            "unexpected error: {error}"
        );
    }
}
