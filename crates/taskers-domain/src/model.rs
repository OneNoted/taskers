use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

use crate::{
    AttentionState, LayoutNode, PaneId, SessionId, SignalEvent, SignalKind, SplitAxis, WindowId,
    WorkspaceId,
};

pub const SESSION_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("window {0} was not found")]
    MissingWindow(WindowId),
    #[error("workspace {0} was not found")]
    MissingWorkspace(WorkspaceId),
    #[error("pane {0} was not found")]
    MissingPane(PaneId),
    #[error("workspace {workspace_id} does not contain pane {pane_id}")]
    PaneNotInWorkspace {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    Terminal,
    Browser,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneMetadata {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Vec<u16>,
    pub agent_kind: Option<String>,
    pub last_signal_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneMetadataPatch {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Option<Vec<u16>>,
    pub agent_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRecord {
    pub id: PaneId,
    pub kind: PaneKind,
    pub metadata: PaneMetadata,
    pub attention: AttentionState,
    pub session_id: SessionId,
    pub command: Option<Vec<String>>,
}

impl PaneRecord {
    pub fn new(kind: PaneKind) -> Self {
        Self {
            id: PaneId::new(),
            kind,
            metadata: PaneMetadata::default(),
            attention: AttentionState::Normal,
            session_id: SessionId::new(),
            command: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationItem {
    pub pane_id: PaneId,
    pub state: AttentionState,
    pub message: String,
    pub created_at: OffsetDateTime,
    pub cleared_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityItem {
    pub workspace_id: WorkspaceId,
    pub pane_id: PaneId,
    pub state: AttentionState,
    pub message: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub label: String,
    pub layout: LayoutNode,
    pub panes: IndexMap<PaneId, PaneRecord>,
    pub active_pane: PaneId,
    pub notifications: Vec<NotificationItem>,
}

impl Workspace {
    pub fn bootstrap(label: impl Into<String>) -> Self {
        let first_pane = PaneRecord::new(PaneKind::Terminal);
        let active_pane = first_pane.id;
        let mut panes = IndexMap::new();
        panes.insert(active_pane, first_pane);

        Self {
            id: WorkspaceId::new(),
            label: label.into(),
            layout: LayoutNode::leaf(active_pane),
            panes,
            active_pane,
            notifications: Vec::new(),
        }
    }

    pub fn repo_hint(&self) -> Option<&str> {
        self.panes
            .values()
            .find_map(|pane| pane.metadata.repo_name.as_deref())
    }

    pub fn attention_counts(&self) -> BTreeMap<AttentionState, usize> {
        let mut counts = BTreeMap::new();
        for pane in self.panes.values() {
            *counts.entry(pane.attention).or_insert(0) += 1;
        }
        counts
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub label: String,
    pub active_pane: PaneId,
    pub repo_hint: Option<String>,
    pub counts_by_attention: BTreeMap<AttentionState, usize>,
    pub highest_attention: AttentionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRecord {
    pub id: WindowId,
    pub workspace_order: Vec<WorkspaceId>,
    pub active_workspace: WorkspaceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppModel {
    pub active_window: WindowId,
    pub windows: IndexMap<WindowId, WindowRecord>,
    pub workspaces: IndexMap<WorkspaceId, Workspace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSession {
    pub schema_version: u32,
    pub captured_at: OffsetDateTime,
    pub model: AppModel,
}

impl AppModel {
    pub fn new(label: impl Into<String>) -> Self {
        let window_id = WindowId::new();
        let workspace = Workspace::bootstrap(label);
        let workspace_id = workspace.id;

        let mut windows = IndexMap::new();
        windows.insert(
            window_id,
            WindowRecord {
                id: window_id,
                workspace_order: vec![workspace_id],
                active_workspace: workspace_id,
            },
        );

        let mut workspaces = IndexMap::new();
        workspaces.insert(workspace_id, workspace);

        Self {
            active_window: window_id,
            windows,
            workspaces,
        }
    }

    pub fn demo() -> Self {
        let mut model = Self::new("Repo A");
        let primary_workspace = model
            .active_workspace_id()
            .unwrap_or_else(|| WorkspaceId::new());
        let first_pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .unwrap_or_else(PaneId::new);

        let _ = model.update_pane_metadata(
            first_pane,
            PaneMetadataPatch {
                title: Some("Codex".into()),
                cwd: Some("/home/notes/Projects/taskers".into()),
                repo_name: Some("taskers".into()),
                git_branch: Some("main".into()),
                ports: Some(vec![3000]),
                agent_kind: Some("codex".into()),
            },
        );
        let _ = model.apply_signal(
            primary_workspace,
            first_pane,
            SignalEvent::new(
                "demo",
                SignalKind::WaitingInput,
                Some("Waiting for review on workspace bootstrap".into()),
            ),
        );

        let split_pane = model
            .split_pane(primary_workspace, Some(first_pane), SplitAxis::Vertical)
            .unwrap_or(first_pane);
        let _ = model.update_pane_metadata(
            split_pane,
            PaneMetadataPatch {
                title: Some("Claude".into()),
                cwd: Some("/home/notes/Projects/taskers".into()),
                repo_name: Some("taskers".into()),
                git_branch: Some("feature/bootstrap".into()),
                ports: Some(vec![]),
                agent_kind: Some("claude".into()),
            },
        );
        let _ = model.apply_signal(
            primary_workspace,
            split_pane,
            SignalEvent::new(
                "demo",
                SignalKind::Progress,
                Some("Running long task".into()),
            ),
        );

        let second_workspace = model.create_workspace("Docs");
        let second_pane = model
            .workspaces
            .get(&second_workspace)
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .unwrap_or_else(PaneId::new);
        let _ = model.update_pane_metadata(
            second_pane,
            PaneMetadataPatch {
                title: Some("OpenCode".into()),
                cwd: Some("/home/notes/Documents".into()),
                repo_name: Some("notes".into()),
                git_branch: Some("docs".into()),
                ports: Some(vec![8080, 8081]),
                agent_kind: Some("opencode".into()),
            },
        );
        let _ = model.apply_signal(
            second_workspace,
            second_pane,
            SignalEvent::new(
                "demo",
                SignalKind::Completed,
                Some("Draft completed, ready for merge".into()),
            ),
        );
        let _ = model.switch_workspace(model.active_window, second_workspace);

        model
    }

    pub fn active_window(&self) -> Option<&WindowRecord> {
        self.windows.get(&self.active_window)
    }

    pub fn active_workspace_id(&self) -> Option<WorkspaceId> {
        self.active_window().map(|window| window.active_workspace)
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.active_workspace_id()
            .and_then(|workspace_id| self.workspaces.get(&workspace_id))
    }

    pub fn create_workspace(&mut self, label: impl Into<String>) -> WorkspaceId {
        let workspace = Workspace::bootstrap(label);
        let workspace_id = workspace.id;
        self.workspaces.insert(workspace_id, workspace);
        if let Some(window) = self.windows.get_mut(&self.active_window) {
            window.workspace_order.push(workspace_id);
            window.active_workspace = workspace_id;
        }
        workspace_id
    }

    pub fn rename_workspace(
        &mut self,
        workspace_id: WorkspaceId,
        label: impl Into<String>,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        workspace.label = label.into();
        Ok(())
    }

    pub fn switch_workspace(
        &mut self,
        window_id: WindowId,
        workspace_id: WorkspaceId,
    ) -> Result<(), DomainError> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(DomainError::MissingWindow(window_id))?;
        if !window.workspace_order.contains(&workspace_id) {
            return Err(DomainError::MissingWorkspace(workspace_id));
        }
        window.active_workspace = workspace_id;
        Ok(())
    }

    pub fn split_pane(
        &mut self,
        workspace_id: WorkspaceId,
        target_pane: Option<PaneId>,
        axis: SplitAxis,
    ) -> Result<PaneId, DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;

        let target = target_pane.unwrap_or(workspace.active_pane);
        if !workspace.panes.contains_key(&target) {
            return Err(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id: target,
            });
        }

        let new_pane = PaneRecord::new(PaneKind::Terminal);
        let new_pane_id = new_pane.id;
        workspace.panes.insert(new_pane_id, new_pane);
        workspace.layout.split_leaf(target, axis, new_pane_id, 50);
        workspace.active_pane = new_pane_id;

        Ok(new_pane_id)
    }

    pub fn focus_pane(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;

        if !workspace.panes.contains_key(&pane_id) {
            return Err(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id,
            });
        }

        workspace.active_pane = pane_id;
        Ok(())
    }

    pub fn update_pane_metadata(
        &mut self,
        pane_id: PaneId,
        patch: PaneMetadataPatch,
    ) -> Result<(), DomainError> {
        let pane = self
            .workspaces
            .values_mut()
            .find_map(|workspace| workspace.panes.get_mut(&pane_id))
            .ok_or(DomainError::MissingPane(pane_id))?;

        if patch.title.is_some() {
            pane.metadata.title = patch.title;
        }
        if patch.cwd.is_some() {
            pane.metadata.cwd = patch.cwd;
        }
        if patch.repo_name.is_some() {
            pane.metadata.repo_name = patch.repo_name;
        }
        if patch.git_branch.is_some() {
            pane.metadata.git_branch = patch.git_branch;
        }
        if let Some(ports) = patch.ports {
            pane.metadata.ports = ports;
        }
        if patch.agent_kind.is_some() {
            pane.metadata.agent_kind = patch.agent_kind;
        }

        Ok(())
    }

    pub fn apply_signal(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        event: SignalEvent,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let pane = workspace
            .panes
            .get_mut(&pane_id)
            .ok_or(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id,
            })?;

        pane.metadata.last_signal_at = Some(event.timestamp);
        pane.attention = map_signal_to_attention(&event.kind);

        if let Some(message) = event.message {
            workspace.notifications.push(NotificationItem {
                pane_id,
                state: pane.attention,
                message,
                created_at: event.timestamp,
                cleared_at: None,
            });
        }

        Ok(())
    }

    pub fn close_pane(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;

        if !workspace.panes.contains_key(&pane_id) {
            return Err(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id,
            });
        }

        if workspace.panes.len() <= 1 {
            return self.close_workspace(workspace_id);
        }

        let workspace = self.workspaces.get_mut(&workspace_id).unwrap();
        workspace.layout.remove_leaf(pane_id);
        workspace.panes.shift_remove(&pane_id);

        if workspace.active_pane == pane_id {
            workspace.active_pane = *workspace
                .panes
                .first()
                .map(|(id, _)| id)
                .expect("at least one pane remains");
        }

        Ok(())
    }

    pub fn close_workspace(&mut self, workspace_id: WorkspaceId) -> Result<(), DomainError> {
        if !self.workspaces.contains_key(&workspace_id) {
            return Err(DomainError::MissingWorkspace(workspace_id));
        }

        if self.workspaces.len() <= 1 {
            self.create_workspace("Workspace 1");
        }

        self.workspaces.shift_remove(&workspace_id);

        for window in self.windows.values_mut() {
            window.workspace_order.retain(|id| *id != workspace_id);
            if window.active_workspace == workspace_id {
                if let Some(first) = window.workspace_order.first() {
                    window.active_workspace = *first;
                }
            }
        }

        Ok(())
    }

    pub fn workspace_summaries(
        &self,
        window_id: WindowId,
    ) -> Result<Vec<WorkspaceSummary>, DomainError> {
        let window = self
            .windows
            .get(&window_id)
            .ok_or(DomainError::MissingWindow(window_id))?;

        let summaries = window
            .workspace_order
            .iter()
            .filter_map(|workspace_id| self.workspaces.get(workspace_id))
            .map(|workspace| {
                let counts = workspace.attention_counts();
                let highest_attention = workspace
                    .panes
                    .values()
                    .map(|pane| pane.attention)
                    .max_by_key(|attention| attention.rank())
                    .unwrap_or(AttentionState::Normal);

                WorkspaceSummary {
                    workspace_id: workspace.id,
                    label: workspace.label.clone(),
                    active_pane: workspace.active_pane,
                    repo_hint: workspace.repo_hint().map(str::to_owned),
                    counts_by_attention: counts,
                    highest_attention,
                }
            })
            .collect();

        Ok(summaries)
    }

    pub fn activity_items(&self) -> Vec<ActivityItem> {
        let mut items = self
            .workspaces
            .values()
            .flat_map(|workspace| {
                workspace
                    .notifications
                    .iter()
                    .map(move |notification| ActivityItem {
                        workspace_id: workspace.id,
                        pane_id: notification.pane_id,
                        state: notification.state,
                        message: notification.message.clone(),
                        created_at: notification.created_at,
                    })
            })
            .collect::<Vec<_>>();

        items.sort_by(|left, right| right.created_at.cmp(&left.created_at));
        items
    }

    pub fn snapshot(&self) -> PersistedSession {
        PersistedSession {
            schema_version: SESSION_SCHEMA_VERSION,
            captured_at: OffsetDateTime::now_utc(),
            model: self.clone(),
        }
    }
}

fn map_signal_to_attention(kind: &SignalKind) -> AttentionState {
    match kind {
        SignalKind::Started | SignalKind::Progress => AttentionState::Busy,
        SignalKind::Completed => AttentionState::Completed,
        SignalKind::WaitingInput => AttentionState::WaitingInput,
        SignalKind::Error => AttentionState::Error,
        SignalKind::Notification => AttentionState::Busy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_pane_updates_layout_and_focus() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .expect("pane");

        let new_pane = model
            .split_pane(workspace_id, Some(first_pane), SplitAxis::Vertical)
            .expect("split works");
        let workspace = model
            .workspaces
            .get(&workspace_id)
            .expect("workspace exists");

        assert_eq!(workspace.active_pane, new_pane);
        assert_eq!(workspace.layout.leaves(), vec![first_pane, new_pane]);
    }

    #[test]
    fn signals_flow_into_activity_and_summary() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .expect("pane");

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::new(
                    "test",
                    SignalKind::WaitingInput,
                    Some("Need approval".into()),
                ),
            )
            .expect("signal applied");

        let summaries = model
            .workspace_summaries(model.active_window)
            .expect("summary available");
        let summary = summaries.first().expect("summary");

        assert_eq!(summary.highest_attention, AttentionState::WaitingInput);
        assert_eq!(
            summary
                .counts_by_attention
                .get(&AttentionState::WaitingInput)
                .copied(),
            Some(1)
        );
        assert_eq!(model.activity_items().len(), 1);
    }

    #[test]
    fn focus_pane_updates_active_pane() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .expect("pane");
        let second_pane = model
            .split_pane(workspace_id, Some(first_pane), SplitAxis::Horizontal)
            .expect("split");

        model
            .focus_pane(workspace_id, first_pane)
            .expect("focus first pane");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        assert_eq!(workspace.active_pane, first_pane);
        assert_ne!(workspace.active_pane, second_pane);
    }

    #[test]
    fn persisted_session_roundtrips() {
        let model = AppModel::demo();
        let snapshot = model.snapshot();
        let encoded = serde_json::to_string_pretty(&snapshot).expect("serialize");
        let decoded: PersistedSession = serde_json::from_str(&encoded).expect("deserialize");

        assert_eq!(decoded.schema_version, SESSION_SCHEMA_VERSION);
        assert_eq!(decoded.model.workspaces.len(), model.workspaces.len());
    }
}
