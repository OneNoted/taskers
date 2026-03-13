use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use time::OffsetDateTime;

use crate::{
    AttentionState, Direction, LayoutNode, PaneId, SessionId, SignalEvent, SignalKind, SplitAxis,
    WindowId, WorkspaceId, WorkspaceWindowId,
};

pub const SESSION_SCHEMA_VERSION: u32 = 2;
pub const DEFAULT_WORKSPACE_WINDOW_WIDTH: i32 = 1280;
pub const DEFAULT_WORKSPACE_WINDOW_HEIGHT: i32 = 860;
pub const DEFAULT_WORKSPACE_WINDOW_GAP: i32 = 2;
pub const MIN_WORKSPACE_WINDOW_WIDTH: i32 = 720;
pub const MIN_WORKSPACE_WINDOW_HEIGHT: i32 = 420;
pub const KEYBOARD_RESIZE_STEP: i32 = 80;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("window {0} was not found")]
    MissingWindow(WindowId),
    #[error("workspace {0} was not found")]
    MissingWorkspace(WorkspaceId),
    #[error("workspace window {0} was not found")]
    MissingWorkspaceWindow(WorkspaceWindowId),
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceViewport {
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl WindowFrame {
    pub fn root() -> Self {
        Self {
            x: 0,
            y: 0,
            width: DEFAULT_WORKSPACE_WINDOW_WIDTH,
            height: DEFAULT_WORKSPACE_WINDOW_HEIGHT,
        }
    }

    pub fn right(self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(self) -> i32 {
        self.y + self.height
    }

    pub fn center_x(self) -> i32 {
        self.x + (self.width / 2)
    }

    pub fn center_y(self) -> i32 {
        self.y + (self.height / 2)
    }

    pub fn shifted(self, direction: Direction) -> Self {
        match direction {
            Direction::Left => Self {
                x: self.x - self.width - DEFAULT_WORKSPACE_WINDOW_GAP,
                ..self
            },
            Direction::Right => Self {
                x: self.x + self.width + DEFAULT_WORKSPACE_WINDOW_GAP,
                ..self
            },
            Direction::Up => Self {
                y: self.y - self.height - DEFAULT_WORKSPACE_WINDOW_GAP,
                ..self
            },
            Direction::Down => Self {
                y: self.y + self.height + DEFAULT_WORKSPACE_WINDOW_GAP,
                ..self
            },
        }
    }

    pub fn resize_by_direction(&mut self, direction: Direction, amount: i32) {
        match direction {
            Direction::Left => {
                self.width = (self.width - amount).max(MIN_WORKSPACE_WINDOW_WIDTH);
            }
            Direction::Right => {
                self.width = (self.width + amount).max(MIN_WORKSPACE_WINDOW_WIDTH);
            }
            Direction::Up => {
                self.height = (self.height - amount).max(MIN_WORKSPACE_WINDOW_HEIGHT);
            }
            Direction::Down => {
                self.height = (self.height + amount).max(MIN_WORKSPACE_WINDOW_HEIGHT);
            }
        }
    }

    pub fn clamp(&mut self) {
        self.width = self.width.max(MIN_WORKSPACE_WINDOW_WIDTH);
        self.height = self.height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
    }

    fn overlaps(self, other: Self) -> bool {
        self.x < other.right()
            && self.right() > other.x
            && self.y < other.bottom()
            && self.bottom() > other.y
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceWindowRecord {
    pub id: WorkspaceWindowId,
    pub frame: WindowFrame,
    pub layout: LayoutNode,
    pub active_pane: PaneId,
}

impl WorkspaceWindowRecord {
    fn new(frame: WindowFrame, pane_id: PaneId) -> Self {
        Self {
            id: WorkspaceWindowId::new(),
            frame,
            layout: LayoutNode::leaf(pane_id),
            active_pane: pane_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub label: String,
    pub windows: IndexMap<WorkspaceWindowId, WorkspaceWindowRecord>,
    pub active_window: WorkspaceWindowId,
    pub panes: IndexMap<PaneId, PaneRecord>,
    pub active_pane: PaneId,
    #[serde(default)]
    pub viewport: WorkspaceViewport,
    pub notifications: Vec<NotificationItem>,
}

impl<'de> Deserialize<'de> for Workspace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let workspace = match WorkspaceSerdeCompat::deserialize(deserializer)? {
            WorkspaceSerdeCompat::Current(current) => current.into_workspace(),
            WorkspaceSerdeCompat::Legacy(legacy) => legacy.into_workspace(),
        };
        Ok(workspace)
    }
}

impl Workspace {
    pub fn bootstrap(label: impl Into<String>) -> Self {
        let first_pane = PaneRecord::new(PaneKind::Terminal);
        let active_pane = first_pane.id;
        let mut panes = IndexMap::new();
        panes.insert(active_pane, first_pane);
        let first_window = WorkspaceWindowRecord::new(WindowFrame::root(), active_pane);
        let active_window = first_window.id;
        let mut windows = IndexMap::new();
        windows.insert(active_window, first_window);

        Self {
            id: WorkspaceId::new(),
            label: label.into(),
            windows,
            active_window,
            panes,
            active_pane,
            viewport: WorkspaceViewport::default(),
            notifications: Vec::new(),
        }
    }

    pub fn active_window_record(&self) -> Option<&WorkspaceWindowRecord> {
        self.windows.get(&self.active_window)
    }

    pub fn active_window_record_mut(&mut self) -> Option<&mut WorkspaceWindowRecord> {
        self.windows.get_mut(&self.active_window)
    }

    pub fn window_for_pane(&self, pane_id: PaneId) -> Option<WorkspaceWindowId> {
        self.windows
            .iter()
            .find_map(|(window_id, window)| window.layout.contains(pane_id).then_some(*window_id))
    }

    fn sync_active_from_window(&mut self, window_id: WorkspaceWindowId) {
        if let Some(window) = self.windows.get(&window_id) {
            self.active_window = window_id;
            self.active_pane = window.active_pane;
        }
    }

    fn focus_window(&mut self, window_id: WorkspaceWindowId) {
        if let Some(window) = self.windows.get(&window_id) {
            self.active_window = window_id;
            self.active_pane = window.active_pane;
        }
    }

    fn focus_pane(&mut self, pane_id: PaneId) -> bool {
        let Some(window_id) = self.window_for_pane(pane_id) else {
            return false;
        };
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.active_pane = pane_id;
        }
        self.sync_active_from_window(window_id);
        true
    }

    fn next_window_frame(&self, source: WindowFrame, direction: Direction) -> WindowFrame {
        let mut candidate = source.shifted(direction);
        while self
            .windows
            .values()
            .any(|window| window.frame.overlaps(candidate))
        {
            candidate = candidate.shifted(direction);
        }
        candidate
    }

    fn top_level_neighbor(
        &self,
        source_window_id: WorkspaceWindowId,
        direction: Direction,
    ) -> Option<WorkspaceWindowId> {
        let source = self.windows.get(&source_window_id)?.frame;
        self.windows
            .iter()
            .filter(|(window_id, _)| **window_id != source_window_id)
            .filter_map(|(window_id, window)| {
                let primary = match direction {
                    Direction::Left => source.center_x() - window.frame.center_x(),
                    Direction::Right => window.frame.center_x() - source.center_x(),
                    Direction::Up => source.center_y() - window.frame.center_y(),
                    Direction::Down => window.frame.center_y() - source.center_y(),
                };
                if primary <= 0 {
                    return None;
                }

                let secondary = match direction {
                    Direction::Left | Direction::Right => {
                        (window.frame.center_y() - source.center_y()).abs()
                    }
                    Direction::Up | Direction::Down => {
                        (window.frame.center_x() - source.center_x()).abs()
                    }
                };
                Some((*window_id, primary, secondary))
            })
            .min_by_key(|(_, primary, secondary)| (*primary, *secondary))
            .map(|(window_id, _, _)| window_id)
    }

    fn fallback_window_after_close(&self, source: WindowFrame) -> Option<WorkspaceWindowId> {
        [
            Direction::Right,
            Direction::Down,
            Direction::Left,
            Direction::Up,
        ]
        .into_iter()
        .find_map(|direction| {
            self.windows
                .iter()
                .filter_map(|(window_id, window)| {
                    let primary = match direction {
                        Direction::Left => source.center_x() - window.frame.center_x(),
                        Direction::Right => window.frame.center_x() - source.center_x(),
                        Direction::Up => source.center_y() - window.frame.center_y(),
                        Direction::Down => window.frame.center_y() - source.center_y(),
                    };
                    if primary <= 0 {
                        return None;
                    }
                    let secondary = match direction {
                        Direction::Left | Direction::Right => {
                            (window.frame.center_y() - source.center_y()).abs()
                        }
                        Direction::Up | Direction::Down => {
                            (window.frame.center_x() - source.center_x()).abs()
                        }
                    };
                    Some((*window_id, primary, secondary))
                })
                .min_by_key(|(_, primary, secondary)| (*primary, *secondary))
                .map(|(window_id, _, _)| window_id)
        })
        .or_else(|| self.windows.first().map(|(window_id, _)| *window_id))
    }

    fn normalize(&mut self) {
        if self.panes.is_empty() {
            let id = self.id;
            let label = self.label.clone();
            *self = Self::bootstrap(label);
            self.id = id;
            return;
        }

        if self.windows.is_empty() {
            let fallback_pane = self
                .panes
                .first()
                .map(|(pane_id, _)| *pane_id)
                .expect("workspace has at least one pane");
            let fallback_window = WorkspaceWindowRecord::new(WindowFrame::root(), fallback_pane);
            self.active_window = fallback_window.id;
            self.active_pane = fallback_pane;
            self.windows.insert(fallback_window.id, fallback_window);
        }

        for window in self.windows.values_mut() {
            if !window.layout.contains(window.active_pane) {
                window.active_pane = window
                    .layout
                    .leaves()
                    .into_iter()
                    .find(|pane_id| self.panes.contains_key(pane_id))
                    .or_else(|| self.panes.first().map(|(pane_id, _)| *pane_id))
                    .expect("workspace has at least one pane");
            }
        }

        if !self.windows.contains_key(&self.active_window) {
            self.active_window = self
                .windows
                .first()
                .map(|(window_id, _)| *window_id)
                .expect("workspace has at least one window");
        }
        if !self
            .windows
            .get(&self.active_window)
            .is_some_and(|window| window.layout.contains(self.active_pane))
        {
            self.active_pane = self
                .windows
                .get(&self.active_window)
                .map(|window| window.active_pane)
                .expect("active window exists");
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
        let primary_workspace = model.active_workspace_id().unwrap_or_else(WorkspaceId::new);
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

        let second_window_pane = model
            .create_workspace_window(primary_workspace, Direction::Right)
            .unwrap_or(first_pane);
        let _ = model.update_pane_metadata(
            second_window_pane,
            PaneMetadataPatch {
                title: Some("Claude".into()),
                cwd: Some("/home/notes/Projects/taskers".into()),
                repo_name: Some("taskers".into()),
                git_branch: Some("feature/bootstrap".into()),
                ports: Some(vec![]),
                agent_kind: Some("claude".into()),
            },
        );
        let split_pane = model
            .split_pane(
                primary_workspace,
                Some(second_window_pane),
                SplitAxis::Vertical,
            )
            .unwrap_or(second_window_pane);
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

    pub fn create_workspace_window(
        &mut self,
        workspace_id: WorkspaceId,
        direction: Direction,
    ) -> Result<PaneId, DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;

        let source_frame = workspace
            .active_window_record()
            .map(|window| window.frame)
            .unwrap_or_else(WindowFrame::root);
        let new_pane = PaneRecord::new(PaneKind::Terminal);
        let new_pane_id = new_pane.id;
        workspace.panes.insert(new_pane_id, new_pane);

        let frame = workspace.next_window_frame(source_frame, direction);
        let new_window = WorkspaceWindowRecord::new(frame, new_pane_id);
        let new_window_id = new_window.id;
        workspace.windows.insert(new_window_id, new_window);
        workspace.sync_active_from_window(new_window_id);

        Ok(new_pane_id)
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

        let window_id = workspace
            .window_for_pane(target)
            .ok_or(DomainError::MissingPane(target))?;
        let new_pane = PaneRecord::new(PaneKind::Terminal);
        let new_pane_id = new_pane.id;
        workspace.panes.insert(new_pane_id, new_pane);

        if let Some(window) = workspace.windows.get_mut(&window_id) {
            window.layout.split_leaf(target, axis, new_pane_id, 500);
            window.active_pane = new_pane_id;
        }
        workspace.sync_active_from_window(window_id);

        Ok(new_pane_id)
    }

    pub fn focus_workspace_window(
        &mut self,
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        if !workspace.windows.contains_key(&workspace_window_id) {
            return Err(DomainError::MissingWorkspaceWindow(workspace_window_id));
        }
        workspace.focus_window(workspace_window_id);
        Ok(())
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

        workspace.focus_pane(pane_id);
        Ok(())
    }

    pub fn focus_pane_direction(
        &mut self,
        workspace_id: WorkspaceId,
        direction: Direction,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let active_window_id = workspace.active_window;

        if let Some(next_window_id) = workspace.top_level_neighbor(active_window_id, direction) {
            workspace.focus_window(next_window_id);
            return Ok(());
        }

        let next_pane = workspace
            .windows
            .get(&active_window_id)
            .and_then(|window| window.layout.focus_neighbor(window.active_pane, direction));
        if let Some(next_pane) = next_pane {
            if let Some(window) = workspace.windows.get_mut(&active_window_id) {
                window.active_pane = next_pane;
            }
            workspace.sync_active_from_window(active_window_id);
        }

        Ok(())
    }

    pub fn resize_active_window(
        &mut self,
        workspace_id: WorkspaceId,
        direction: Direction,
        amount: i32,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let active_window = workspace.active_window;
        let window = workspace
            .active_window_record_mut()
            .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
        window.frame.resize_by_direction(direction, amount);
        window.frame.clamp();
        Ok(())
    }

    pub fn resize_active_pane_split(
        &mut self,
        workspace_id: WorkspaceId,
        direction: Direction,
        amount: i32,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let active_window_id = workspace.active_window;
        let active_pane = workspace.active_pane;
        let window = workspace
            .windows
            .get_mut(&active_window_id)
            .ok_or(DomainError::MissingWorkspaceWindow(active_window_id))?;
        window.layout.resize_leaf(active_pane, direction, amount);
        Ok(())
    }

    pub fn set_workspace_window_frame(
        &mut self,
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        mut frame: WindowFrame,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let window = workspace
            .windows
            .get_mut(&workspace_window_id)
            .ok_or(DomainError::MissingWorkspaceWindow(workspace_window_id))?;
        frame.clamp();
        window.frame = frame;
        Ok(())
    }

    pub fn set_window_split_ratio(
        &mut self,
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        path: &[bool],
        ratio: u16,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let window = workspace
            .windows
            .get_mut(&workspace_window_id)
            .ok_or(DomainError::MissingWorkspaceWindow(workspace_window_id))?;
        window.layout.set_ratio_at_path(path, ratio);
        Ok(())
    }

    pub fn set_workspace_viewport(
        &mut self,
        workspace_id: WorkspaceId,
        viewport: WorkspaceViewport,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        workspace.viewport = viewport;
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
        {
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
        }

        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let window_id = workspace
            .window_for_pane(pane_id)
            .ok_or(DomainError::MissingPane(pane_id))?;

        let window_leaf_count = workspace
            .windows
            .get(&window_id)
            .map(|window| window.layout.leaves().len())
            .unwrap_or_default();
        if window_leaf_count <= 1 && workspace.windows.len() > 1 {
            let source_frame = workspace
                .windows
                .get(&window_id)
                .map(|window| window.frame)
                .expect("window exists");
            workspace.windows.shift_remove(&window_id);
            workspace.panes.shift_remove(&pane_id);
            workspace
                .notifications
                .retain(|item| item.pane_id != pane_id);
            if let Some(next_window_id) = workspace.fallback_window_after_close(source_frame) {
                workspace.sync_active_from_window(next_window_id);
            }
            return Ok(());
        }

        if let Some(window) = workspace.windows.get_mut(&window_id) {
            let fallback_focus = close_layout_pane(window, pane_id)
                .or_else(|| window.layout.leaves().into_iter().next())
                .expect("window should retain at least one pane");
            window.active_pane = fallback_focus;
        }
        workspace.panes.shift_remove(&pane_id);
        workspace
            .notifications
            .retain(|item| item.pane_id != pane_id);

        if workspace.active_window == window_id {
            workspace.sync_active_from_window(window_id);
        } else if workspace.active_pane == pane_id {
            workspace.sync_active_from_window(workspace.active_window);
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
            if window.active_workspace == workspace_id
                && let Some(first) = window.workspace_order.first()
            {
                window.active_workspace = *first;
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WorkspaceSerdeCompat {
    Current(CurrentWorkspaceSerde),
    Legacy(LegacyWorkspaceSerde),
}

#[derive(Debug, Deserialize)]
struct CurrentWorkspaceSerde {
    id: WorkspaceId,
    label: String,
    windows: IndexMap<WorkspaceWindowId, WorkspaceWindowRecord>,
    active_window: WorkspaceWindowId,
    panes: IndexMap<PaneId, PaneRecord>,
    active_pane: PaneId,
    #[serde(default)]
    viewport: WorkspaceViewport,
    #[serde(default)]
    notifications: Vec<NotificationItem>,
}

impl CurrentWorkspaceSerde {
    fn into_workspace(self) -> Workspace {
        let mut workspace = Workspace {
            id: self.id,
            label: self.label,
            windows: self.windows,
            active_window: self.active_window,
            panes: self.panes,
            active_pane: self.active_pane,
            viewport: self.viewport,
            notifications: self.notifications,
        };
        workspace.normalize();
        workspace
    }
}

#[derive(Debug, Deserialize)]
struct LegacyWorkspaceSerde {
    id: WorkspaceId,
    label: String,
    layout: LegacyWorkspaceLayout,
    panes: IndexMap<PaneId, PaneRecord>,
    active_pane: PaneId,
    #[serde(default)]
    notifications: Vec<NotificationItem>,
}

impl LegacyWorkspaceSerde {
    fn into_workspace(self) -> Workspace {
        let mut windows = IndexMap::new();
        let mut viewport = WorkspaceViewport::default();
        let mut active_window = None;
        let preferred_active_pane = if self.panes.contains_key(&self.active_pane) {
            self.active_pane
        } else {
            self.panes
                .first()
                .map(|(pane_id, _)| *pane_id)
                .unwrap_or_else(PaneId::new)
        };

        match self.layout {
            LegacyWorkspaceLayout::SplitTree(layout) => {
                let active_pane = active_pane_for_layout(&layout, preferred_active_pane);
                let window = WorkspaceWindowRecord {
                    id: WorkspaceWindowId::new(),
                    frame: WindowFrame::root(),
                    layout,
                    active_pane,
                };
                active_window = Some(window.id);
                windows.insert(window.id, window);
            }
            LegacyWorkspaceLayout::Scrollable(scrollable) => {
                viewport = scrollable.viewport;
                for (index, column) in scrollable.columns.into_iter().enumerate() {
                    let Some(layout) = layout_from_pane_stack(&column.panes) else {
                        continue;
                    };
                    let active_pane = active_pane_for_layout(&layout, preferred_active_pane);
                    let frame = WindowFrame {
                        x: index as i32
                            * (DEFAULT_WORKSPACE_WINDOW_WIDTH + DEFAULT_WORKSPACE_WINDOW_GAP),
                        y: 0,
                        width: DEFAULT_WORKSPACE_WINDOW_WIDTH,
                        height: DEFAULT_WORKSPACE_WINDOW_HEIGHT,
                    };
                    let window = WorkspaceWindowRecord {
                        id: WorkspaceWindowId::new(),
                        frame,
                        layout,
                        active_pane,
                    };
                    if window.layout.contains(preferred_active_pane) {
                        active_window = Some(window.id);
                    }
                    windows.insert(window.id, window);
                }
            }
        }

        let mut workspace = Workspace {
            id: self.id,
            label: self.label,
            windows,
            active_window: active_window.unwrap_or_else(WorkspaceWindowId::new),
            panes: self.panes,
            active_pane: preferred_active_pane,
            viewport,
            notifications: self.notifications,
        };
        workspace.normalize();
        workspace
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LegacyWorkspaceLayout {
    Scrollable(LegacyScrollableLayout),
    SplitTree(LayoutNode),
}

#[derive(Debug, Deserialize)]
struct LegacyScrollableLayout {
    #[serde(rename = "kind")]
    _kind: String,
    columns: Vec<LegacyPaneColumn>,
    #[serde(default)]
    viewport: WorkspaceViewport,
}

#[derive(Debug, Deserialize)]
struct LegacyPaneColumn {
    panes: Vec<PaneId>,
}

fn active_pane_for_layout(layout: &LayoutNode, preferred: PaneId) -> PaneId {
    if layout.contains(preferred) {
        preferred
    } else {
        layout
            .leaves()
            .into_iter()
            .next()
            .expect("legacy layout should contain at least one pane")
    }
}

fn layout_from_pane_stack(panes: &[PaneId]) -> Option<LayoutNode> {
    let (first, rest) = panes.split_first()?;
    let mut layout = LayoutNode::leaf(*first);
    for pane_id in rest {
        layout = LayoutNode::Split {
            axis: SplitAxis::Vertical,
            ratio: 500,
            first: Box::new(layout),
            second: Box::new(LayoutNode::leaf(*pane_id)),
        };
    }
    Some(layout)
}

fn close_layout_pane(window: &mut WorkspaceWindowRecord, pane_id: PaneId) -> Option<PaneId> {
    let fallback = [
        Direction::Right,
        Direction::Down,
        Direction::Left,
        Direction::Up,
    ]
    .into_iter()
    .find_map(|direction| window.layout.focus_neighbor(pane_id, direction))
    .or_else(|| {
        window
            .layout
            .leaves()
            .into_iter()
            .find(|candidate| *candidate != pane_id)
    });
    let removed = window.layout.remove_leaf(pane_id);
    removed.then_some(fallback).flatten()
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
    use serde_json::json;

    use super::*;

    #[test]
    fn creating_workspace_windows_updates_focus_and_frame() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_window = model
            .active_workspace()
            .and_then(|workspace| workspace.active_window_record().map(|window| window.frame))
            .expect("window");

        let new_pane = model
            .create_workspace_window(workspace_id, Direction::Right)
            .expect("window created");
        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let active_window = workspace.active_window_record().expect("active window");

        assert_eq!(workspace.windows.len(), 2);
        assert_eq!(workspace.active_pane, new_pane);
        assert_eq!(
            active_window.frame.x,
            first_window.x + first_window.width + DEFAULT_WORKSPACE_WINDOW_GAP
        );
        assert_eq!(active_window.frame.y, first_window.y);
    }

    #[test]
    fn split_pane_updates_inner_layout_and_focus() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .expect("pane");

        let new_pane = model
            .split_pane(workspace_id, Some(first_pane), SplitAxis::Vertical)
            .expect("split works");
        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let active_window = workspace.active_window_record().expect("window");

        assert_eq!(workspace.active_pane, new_pane);
        assert_eq!(active_window.layout.leaves(), vec![first_pane, new_pane]);
    }

    #[test]
    fn directional_focus_prefers_top_level_windows_and_restores_inner_focus() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.first().map(|(pane_id, _)| *pane_id))
            .expect("pane");
        let right_window_pane = model
            .create_workspace_window(workspace_id, Direction::Right)
            .expect("window");
        let lower_right_pane = model
            .split_pane(workspace_id, Some(right_window_pane), SplitAxis::Vertical)
            .expect("split");

        model
            .focus_pane(workspace_id, right_window_pane)
            .expect("focus old pane in right window");
        model
            .focus_pane(workspace_id, first_pane)
            .expect("focus left window");
        model
            .focus_pane_direction(workspace_id, Direction::Right)
            .expect("move right");

        assert_eq!(
            model
                .workspaces
                .get(&workspace_id)
                .expect("workspace")
                .active_pane,
            right_window_pane
        );

        model
            .focus_pane(workspace_id, lower_right_pane)
            .expect("focus lower pane");
        model
            .focus_pane_direction(workspace_id, Direction::Left)
            .expect("move left");
        model
            .focus_pane_direction(workspace_id, Direction::Right)
            .expect("move right again");

        assert_eq!(
            model
                .workspaces
                .get(&workspace_id)
                .expect("workspace")
                .active_pane,
            lower_right_pane
        );
    }

    #[test]
    fn closing_last_pane_in_window_removes_window_and_falls_back() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let right_window_pane = model
            .create_workspace_window(workspace_id, Direction::Right)
            .expect("window");

        model
            .close_pane(workspace_id, right_window_pane)
            .expect("close pane");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        assert_eq!(workspace.windows.len(), 1);
        assert!(!workspace.panes.contains_key(&right_window_pane));
        assert_ne!(workspace.active_pane, right_window_pane);
    }

    #[test]
    fn resizing_window_and_split_updates_state() {
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
            .focus_pane(workspace_id, second_pane)
            .expect("focus second pane");
        model
            .resize_active_pane_split(workspace_id, Direction::Right, 60)
            .expect("resize split");
        model
            .resize_active_window(workspace_id, Direction::Right, 120)
            .expect("resize window");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let window = workspace.active_window_record().expect("window");
        let LayoutNode::Split { ratio, .. } = &window.layout else {
            panic!("expected split layout");
        };
        assert_eq!(*ratio, 440);
        assert_eq!(window.frame.width, DEFAULT_WORKSPACE_WINDOW_WIDTH + 120);
    }

    #[test]
    fn legacy_scrollable_layouts_deserialize_into_workspace_windows() {
        let workspace_id = WorkspaceId::new();
        let window_id = WindowId::new();
        let left_pane = PaneRecord::new(PaneKind::Terminal);
        let right_pane = PaneRecord::new(PaneKind::Terminal);

        let encoded = json!({
            "schema_version": 1,
            "captured_at": OffsetDateTime::now_utc(),
            "model": {
                "active_window": window_id,
                "windows": {
                    window_id.to_string(): {
                        "id": window_id,
                        "workspace_order": [workspace_id],
                        "active_workspace": workspace_id
                    }
                },
                "workspaces": {
                    workspace_id.to_string(): {
                        "id": workspace_id,
                        "label": "Main",
                        "layout": {
                            "kind": "scrollable_tiling",
                            "columns": [
                                {"panes": [left_pane.id]},
                                {"panes": [right_pane.id]}
                            ],
                            "viewport": {"x": 64, "y": 24}
                        },
                        "panes": {
                            left_pane.id.to_string(): left_pane,
                            right_pane.id.to_string(): right_pane
                        },
                        "active_pane": right_pane.id,
                        "notifications": []
                    }
                }
            }
        });

        let decoded: PersistedSession =
            serde_json::from_value(encoded).expect("legacy session should deserialize");
        let workspace = decoded
            .model
            .workspaces
            .get(&workspace_id)
            .expect("workspace exists");

        assert_eq!(workspace.windows.len(), 2);
        assert_eq!(workspace.viewport.x, 64);
        assert_eq!(workspace.viewport.y, 24);
        assert_eq!(workspace.active_pane, right_pane.id);
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
    fn persisted_session_roundtrips() {
        let model = AppModel::demo();
        let snapshot = model.snapshot();
        let encoded = serde_json::to_string_pretty(&snapshot).expect("serialize");
        let decoded: PersistedSession = serde_json::from_str(&encoded).expect("deserialize");

        assert_eq!(decoded.schema_version, SESSION_SCHEMA_VERSION);
        assert_eq!(decoded.model.workspaces.len(), model.workspaces.len());
    }
}
