use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use time::{Duration, OffsetDateTime};

use crate::{
    AttentionState, Direction, LayoutNode, PaneId, SessionId, SignalEvent, SignalKind, SplitAxis,
    SurfaceId, WindowId, WorkspaceColumnId, WorkspaceId, WorkspaceWindowId,
};

pub const SESSION_SCHEMA_VERSION: u32 = 4;
pub const DEFAULT_WORKSPACE_WINDOW_WIDTH: i32 = 1280;
pub const DEFAULT_WORKSPACE_WINDOW_HEIGHT: i32 = 860;
pub const DEFAULT_WORKSPACE_WINDOW_GAP: i32 = 2;
pub const MIN_WORKSPACE_WINDOW_WIDTH: i32 = 720;
pub const MIN_WORKSPACE_WINDOW_HEIGHT: i32 = 420;
pub const KEYBOARD_RESIZE_STEP: i32 = 80;

fn split_top_level_extent(extent: i32, min_extent: i32) -> (i32, i32) {
    let extent = extent.max(min_extent);
    if extent < min_extent * 2 {
        return (min_extent, min_extent);
    }

    let retained_extent = (extent + 1) / 2;
    let new_extent = extent - retained_extent;
    (retained_extent.max(min_extent), new_extent.max(min_extent))
}

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("window {0} was not found")]
    MissingWindow(WindowId),
    #[error("workspace {0} was not found")]
    MissingWorkspace(WorkspaceId),
    #[error("workspace column {0} was not found")]
    MissingWorkspaceColumn(WorkspaceColumnId),
    #[error("workspace window {0} was not found")]
    MissingWorkspaceWindow(WorkspaceWindowId),
    #[error("pane {0} was not found")]
    MissingPane(PaneId),
    #[error("surface {0} was not found")]
    MissingSurface(SurfaceId),
    #[error("workspace {workspace_id} does not contain pane {pane_id}")]
    PaneNotInWorkspace {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    },
    #[error("workspace {workspace_id} pane {pane_id} does not contain surface {surface_id}")]
    SurfaceNotInPane {
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneKind {
    Terminal,
    Browser,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressState {
    /// Progress as permille (0–1000).
    pub value: u16,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrStatus {
    Open,
    Draft,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestState {
    pub number: u32,
    pub title: String,
    pub status: PrStatus,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneMetadata {
    pub title: Option<String>,
    #[serde(default)]
    pub agent_title: Option<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Vec<u16>,
    pub agent_kind: Option<String>,
    #[serde(default)]
    pub agent_active: bool,
    pub last_signal_at: Option<OffsetDateTime>,
    #[serde(default)]
    pub progress: Option<ProgressState>,
    #[serde(default)]
    pub pull_requests: Vec<PullRequestState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneMetadataPatch {
    pub title: Option<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub repo_name: Option<String>,
    pub git_branch: Option<String>,
    pub ports: Option<Vec<u16>>,
    pub agent_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceRecord {
    pub id: SurfaceId,
    pub kind: PaneKind,
    pub metadata: PaneMetadata,
    pub attention: AttentionState,
    pub session_id: SessionId,
    pub command: Option<Vec<String>>,
}

impl SurfaceRecord {
    pub fn new(kind: PaneKind) -> Self {
        Self {
            id: SurfaceId::new(),
            kind,
            metadata: PaneMetadata::default(),
            attention: AttentionState::Normal,
            session_id: SessionId::new(),
            command: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRecord {
    pub id: PaneId,
    pub surfaces: IndexMap<SurfaceId, SurfaceRecord>,
    pub active_surface: SurfaceId,
}

impl PaneRecord {
    pub fn new(kind: PaneKind) -> Self {
        let surface = SurfaceRecord::new(kind);
        let active_surface = surface.id;
        let mut surfaces = IndexMap::new();
        surfaces.insert(active_surface, surface);
        Self {
            id: PaneId::new(),
            surfaces,
            active_surface,
        }
    }

    pub fn active_surface(&self) -> Option<&SurfaceRecord> {
        self.surfaces.get(&self.active_surface)
    }

    pub fn active_surface_mut(&mut self) -> Option<&mut SurfaceRecord> {
        self.surfaces.get_mut(&self.active_surface)
    }

    pub fn active_metadata(&self) -> Option<&PaneMetadata> {
        self.active_surface().map(|surface| &surface.metadata)
    }

    pub fn active_metadata_mut(&mut self) -> Option<&mut PaneMetadata> {
        self.active_surface_mut()
            .map(|surface| &mut surface.metadata)
    }

    pub fn active_kind(&self) -> Option<PaneKind> {
        self.active_surface().map(|surface| surface.kind.clone())
    }

    pub fn active_attention(&self) -> AttentionState {
        self.active_surface()
            .map(|surface| surface.attention)
            .unwrap_or(AttentionState::Normal)
    }

    pub fn active_session_id(&self) -> Option<SessionId> {
        self.active_surface().map(|surface| surface.session_id)
    }

    pub fn active_command(&self) -> Option<&[String]> {
        self.active_surface()
            .and_then(|surface| surface.command.as_deref())
    }

    pub fn highest_attention(&self) -> AttentionState {
        self.surfaces
            .values()
            .map(|surface| surface.attention)
            .max_by_key(|attention| attention.rank())
            .unwrap_or(AttentionState::Normal)
    }

    pub fn surface_ids(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.surfaces.keys().copied()
    }

    fn insert_surface(&mut self, surface: SurfaceRecord) {
        self.active_surface = surface.id;
        self.surfaces.insert(surface.id, surface);
    }

    fn focus_surface(&mut self, surface_id: SurfaceId) -> bool {
        if self.surfaces.contains_key(&surface_id) {
            self.active_surface = surface_id;
            true
        } else {
            false
        }
    }

    fn move_surface(&mut self, surface_id: SurfaceId, to_index: usize) -> bool {
        let Some(from_index) = self.surfaces.get_index_of(&surface_id) else {
            return false;
        };

        let last_index = self.surfaces.len().saturating_sub(1);
        let target_index = to_index.min(last_index);
        if from_index == target_index {
            return true;
        }

        self.surfaces.move_index(from_index, target_index);
        true
    }

    fn normalize(&mut self) {
        if self.surfaces.is_empty() {
            let replacement = SurfaceRecord::new(PaneKind::Terminal);
            self.active_surface = replacement.id;
            self.surfaces.insert(replacement.id, replacement);
            return;
        }

        if !self.surfaces.contains_key(&self.active_surface) {
            self.active_surface = self
                .surfaces
                .first()
                .map(|(surface_id, _)| *surface_id)
                .expect("pane has at least one surface");
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationItem {
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    #[serde(default = "default_notification_kind")]
    pub kind: SignalKind,
    pub state: AttentionState,
    #[serde(default)]
    pub title: Option<String>,
    pub message: String,
    pub created_at: OffsetDateTime,
    pub cleared_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityItem {
    pub workspace_id: WorkspaceId,
    pub workspace_window_id: Option<WorkspaceWindowId>,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub kind: SignalKind,
    pub state: AttentionState,
    pub title: Option<String>,
    pub message: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAgentState {
    Working,
    Waiting,
    Inactive,
}

impl WorkspaceAgentState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Waiting => "Waiting",
            Self::Inactive => "Inactive",
        }
    }

    fn sort_rank(self) -> u8 {
        match self {
            Self::Waiting => 0,
            Self::Working => 1,
            Self::Inactive => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceAgentSummary {
    pub workspace_window_id: WorkspaceWindowId,
    pub pane_id: PaneId,
    pub surface_id: SurfaceId,
    pub agent_kind: String,
    pub title: Option<String>,
    pub state: WorkspaceAgentState,
    pub last_signal_at: Option<OffsetDateTime>,
}

fn default_notification_kind() -> SignalKind {
    SignalKind::Notification
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceWindowRecord {
    pub id: WorkspaceWindowId,
    pub height: i32,
    pub layout: LayoutNode,
    pub active_pane: PaneId,
}

impl WorkspaceWindowRecord {
    fn new(pane_id: PaneId) -> Self {
        Self {
            id: WorkspaceWindowId::new(),
            height: DEFAULT_WORKSPACE_WINDOW_HEIGHT,
            layout: LayoutNode::leaf(pane_id),
            active_pane: pane_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceColumnRecord {
    pub id: WorkspaceColumnId,
    pub width: i32,
    pub window_order: Vec<WorkspaceWindowId>,
    pub active_window: WorkspaceWindowId,
}

impl WorkspaceColumnRecord {
    fn new(window_id: WorkspaceWindowId) -> Self {
        Self {
            id: WorkspaceColumnId::new(),
            width: DEFAULT_WORKSPACE_WINDOW_WIDTH,
            window_order: vec![window_id],
            active_window: window_id,
        }
    }

    fn normalize(&mut self, windows: &IndexMap<WorkspaceWindowId, WorkspaceWindowRecord>) {
        self.width = self.width.max(MIN_WORKSPACE_WINDOW_WIDTH);
        self.window_order
            .retain(|window_id| windows.contains_key(window_id));
        if !self.window_order.contains(&self.active_window)
            && let Some(window_id) = self.window_order.first()
        {
            self.active_window = *window_id;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub label: String,
    pub columns: IndexMap<WorkspaceColumnId, WorkspaceColumnRecord>,
    pub windows: IndexMap<WorkspaceWindowId, WorkspaceWindowRecord>,
    pub active_window: WorkspaceWindowId,
    pub panes: IndexMap<PaneId, PaneRecord>,
    pub active_pane: PaneId,
    #[serde(default)]
    pub viewport: WorkspaceViewport,
    pub notifications: Vec<NotificationItem>,
    #[serde(default)]
    pub custom_color: Option<String>,
}

impl<'de> Deserialize<'de> for Workspace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(CurrentWorkspaceSerde::deserialize(deserializer)?.into_workspace())
    }
}

impl Workspace {
    pub fn bootstrap(label: impl Into<String>) -> Self {
        let first_pane = PaneRecord::new(PaneKind::Terminal);
        let active_pane = first_pane.id;
        let mut panes = IndexMap::new();
        panes.insert(active_pane, first_pane);
        let first_window = WorkspaceWindowRecord::new(active_pane);
        let active_window = first_window.id;
        let mut windows = IndexMap::new();
        windows.insert(active_window, first_window);
        let first_column = WorkspaceColumnRecord::new(active_window);
        let mut columns = IndexMap::new();
        columns.insert(first_column.id, first_column);

        Self {
            id: WorkspaceId::new(),
            label: label.into(),
            columns,
            windows,
            active_window,
            panes,
            active_pane,
            viewport: WorkspaceViewport::default(),
            notifications: Vec::new(),
            custom_color: None,
        }
    }

    pub fn active_window_record(&self) -> Option<&WorkspaceWindowRecord> {
        self.windows.get(&self.active_window)
    }

    pub fn active_window_record_mut(&mut self) -> Option<&mut WorkspaceWindowRecord> {
        self.windows.get_mut(&self.active_window)
    }

    pub fn column_for_window(&self, window_id: WorkspaceWindowId) -> Option<WorkspaceColumnId> {
        self.columns.iter().find_map(|(column_id, column)| {
            column
                .window_order
                .contains(&window_id)
                .then_some(*column_id)
        })
    }

    pub fn active_column_id(&self) -> Option<WorkspaceColumnId> {
        self.column_for_window(self.active_window)
    }

    fn position_for_window(
        &self,
        window_id: WorkspaceWindowId,
    ) -> Option<(WorkspaceColumnId, usize, usize)> {
        self.columns
            .iter()
            .enumerate()
            .find_map(|(column_index, (column_id, column))| {
                column
                    .window_order
                    .iter()
                    .position(|candidate| *candidate == window_id)
                    .map(|window_index| (*column_id, column_index, window_index))
            })
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
            if let Some(column_id) = self.column_for_window(window_id)
                && let Some(column) = self.columns.get_mut(&column_id)
            {
                column.active_window = window_id;
            }
        }
    }

    fn focus_window(&mut self, window_id: WorkspaceWindowId) {
        self.sync_active_from_window(window_id);
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

    fn focus_surface(&mut self, pane_id: PaneId, surface_id: SurfaceId) -> bool {
        let Some(pane) = self.panes.get_mut(&pane_id) else {
            return false;
        };
        if !pane.focus_surface(surface_id) {
            return false;
        }
        self.focus_pane(pane_id)
    }

    fn acknowledge_pane_notifications(&mut self, pane_id: PaneId) {
        let now = OffsetDateTime::now_utc();
        for notification in &mut self.notifications {
            if notification.pane_id == pane_id && notification.cleared_at.is_none() {
                notification.cleared_at = Some(now);
            }
        }
    }

    fn acknowledge_surface_notifications(&mut self, pane_id: PaneId, surface_id: SurfaceId) {
        let now = OffsetDateTime::now_utc();
        for notification in &mut self.notifications {
            if notification.pane_id == pane_id
                && notification.surface_id == surface_id
                && notification.cleared_at.is_none()
            {
                notification.cleared_at = Some(now);
            }
        }
    }

    fn top_level_neighbor(
        &self,
        source_window_id: WorkspaceWindowId,
        direction: Direction,
    ) -> Option<WorkspaceWindowId> {
        let (_, column_index, window_index) = self.position_for_window(source_window_id)?;
        match direction {
            Direction::Left => column_index
                .checked_sub(1)
                .and_then(|index| self.columns.get_index(index))
                .map(|(_, column)| column.active_window),
            Direction::Right => self
                .columns
                .get_index(column_index + 1)
                .map(|(_, column)| column.active_window),
            Direction::Up => self
                .columns
                .get_index(column_index)
                .and_then(|(_, column)| {
                    window_index
                        .checked_sub(1)
                        .and_then(|index| column.window_order.get(index))
                })
                .copied(),
            Direction::Down => self
                .columns
                .get_index(column_index)
                .and_then(|(_, column)| column.window_order.get(window_index + 1))
                .copied(),
        }
    }

    fn fallback_window_after_close(
        &self,
        source_column_index: usize,
        source_window_index: usize,
        same_column_survived: bool,
    ) -> Option<WorkspaceWindowId> {
        if same_column_survived
            && let Some((_, column)) = self.columns.get_index(source_column_index)
        {
            if let Some(window_id) = column.window_order.get(source_window_index) {
                return Some(*window_id);
            }
            if let Some(window_id) = source_window_index
                .checked_sub(1)
                .and_then(|index| column.window_order.get(index))
            {
                return Some(*window_id);
            }
        }

        let right_column_index = if same_column_survived {
            source_column_index + 1
        } else {
            source_column_index
        };
        if let Some((_, column)) = self.columns.get_index(right_column_index)
            && let Some(window_id) = column.window_order.first()
        {
            return Some(*window_id);
        }

        source_column_index
            .checked_sub(1)
            .and_then(|index| self.columns.get_index(index))
            .and_then(|(_, column)| column.window_order.first())
            .copied()
    }

    fn insert_column_at(&mut self, index: usize, column: WorkspaceColumnRecord) {
        let insert_index = index.min(self.columns.len());
        let mut next = IndexMap::with_capacity(self.columns.len() + 1);
        let mut pending = Some(column);
        for (current_index, (column_id, current_column)) in
            std::mem::take(&mut self.columns).into_iter().enumerate()
        {
            if current_index == insert_index
                && let Some(column) = pending.take()
            {
                next.insert(column.id, column);
            }
            next.insert(column_id, current_column);
        }
        if let Some(column) = pending.take() {
            next.insert(column.id, column);
        }
        self.columns = next;
    }

    fn append_missing_windows_to_columns(&mut self) {
        let assigned = self
            .columns
            .values()
            .flat_map(|column| column.window_order.iter().copied())
            .collect::<BTreeSet<_>>();
        for window_id in self.windows.keys().copied().collect::<Vec<_>>() {
            if assigned.contains(&window_id) {
                continue;
            }
            let column = WorkspaceColumnRecord::new(window_id);
            self.columns.insert(column.id, column);
        }
    }

    fn normalize(&mut self) {
        if self.panes.is_empty() {
            let id = self.id;
            let label = self.label.clone();
            *self = Self::bootstrap(label);
            self.id = id;
            return;
        }

        for pane in self.panes.values_mut() {
            pane.normalize();
        }

        if self.windows.is_empty() {
            let fallback_pane = self
                .panes
                .first()
                .map(|(pane_id, _)| *pane_id)
                .expect("workspace has at least one pane");
            let fallback_window = WorkspaceWindowRecord::new(fallback_pane);
            self.active_window = fallback_window.id;
            self.active_pane = fallback_pane;
            self.windows.insert(fallback_window.id, fallback_window);
        }

        for window in self.windows.values_mut() {
            window.height = window.height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
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

        for column in self.columns.values_mut() {
            column.normalize(&self.windows);
        }

        let mut assigned = BTreeSet::new();
        for column in self.columns.values_mut() {
            column
                .window_order
                .retain(|window_id| assigned.insert(*window_id));
            if !column.window_order.contains(&column.active_window)
                && let Some(window_id) = column.window_order.first()
            {
                column.active_window = *window_id;
            }
        }
        self.columns
            .retain(|_, column| !column.window_order.is_empty());
        self.append_missing_windows_to_columns();

        if self.columns.is_empty() {
            let fallback_window_id = self
                .windows
                .first()
                .map(|(window_id, _)| *window_id)
                .expect("workspace has at least one window");
            let column = WorkspaceColumnRecord::new(fallback_window_id);
            self.columns.insert(column.id, column);
        }

        if !self.windows.contains_key(&self.active_window) {
            self.active_window = self
                .columns
                .first()
                .map(|(_, column)| column.active_window)
                .expect("workspace has at least one column");
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
        self.sync_active_from_window(self.active_window);
    }

    pub fn repo_hint(&self) -> Option<&str> {
        self.panes.values().find_map(|pane| {
            pane.active_metadata()
                .and_then(|metadata| metadata.repo_name.as_deref())
        })
    }

    pub fn attention_counts(&self) -> BTreeMap<AttentionState, usize> {
        let mut counts = BTreeMap::new();
        for pane in self.panes.values() {
            for surface in pane.surfaces.values() {
                *counts.entry(surface.attention).or_insert(0) += 1;
            }
        }
        counts
    }

    pub fn active_surface_id(&self) -> Option<SurfaceId> {
        self.panes
            .get(&self.active_pane)
            .map(|pane| pane.active_surface)
    }

    pub fn agent_summaries(&self, now: OffsetDateTime) -> Vec<WorkspaceAgentSummary> {
        let mut summaries = self
            .panes
            .iter()
            .flat_map(|(pane_id, pane)| {
                let workspace_window_id = self.window_for_pane(*pane_id);
                pane.surfaces.values().filter_map(move |surface| {
                    let workspace_window_id = workspace_window_id?;
                    let state = workspace_agent_state(surface, now)?;
                    let agent_kind = surface.metadata.agent_kind.clone()?;
                    Some(WorkspaceAgentSummary {
                        workspace_window_id,
                        pane_id: *pane_id,
                        surface_id: surface.id,
                        agent_kind,
                        title: surface
                            .metadata
                            .agent_title
                            .as_deref()
                            .or(surface.metadata.title.as_deref())
                            .map(str::trim)
                            .filter(|title| !title.is_empty())
                            .map(str::to_owned),
                        state,
                        last_signal_at: surface.metadata.last_signal_at,
                    })
                })
            })
            .collect::<Vec<_>>();

        summaries.sort_by(|left, right| {
            left.state
                .sort_rank()
                .cmp(&right.state.sort_rank())
                .then_with(|| right.last_signal_at.cmp(&left.last_signal_at))
                .then_with(|| left.agent_kind.cmp(&right.agent_kind))
                .then_with(|| left.pane_id.cmp(&right.pane_id))
                .then_with(|| left.surface_id.cmp(&right.surface_id))
        });

        summaries
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceSummary {
    pub workspace_id: WorkspaceId,
    pub label: String,
    pub active_pane: PaneId,
    pub repo_hint: Option<String>,
    pub agent_summaries: Vec<WorkspaceAgentSummary>,
    pub counts_by_attention: BTreeMap<AttentionState, usize>,
    pub highest_attention: AttentionState,
    pub display_attention: AttentionState,
    pub unread_count: usize,
    pub latest_notification: Option<String>,
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
                url: None,
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
                url: None,
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
                url: None,
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

    pub fn reorder_workspaces(
        &mut self,
        window_id: WindowId,
        new_order: Vec<WorkspaceId>,
    ) -> Result<(), DomainError> {
        let window = self
            .windows
            .get_mut(&window_id)
            .ok_or(DomainError::MissingWindow(window_id))?;
        let existing: std::collections::HashSet<_> =
            window.workspace_order.iter().copied().collect();
        let proposed: std::collections::HashSet<_> = new_order.iter().copied().collect();
        if existing != proposed {
            return Ok(());
        }
        window.workspace_order = new_order;
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
        let new_pane = PaneRecord::new(PaneKind::Terminal);
        let new_pane_id = new_pane.id;
        workspace.panes.insert(new_pane_id, new_pane);

        let new_window = WorkspaceWindowRecord::new(new_pane_id);
        let new_window_id = new_window.id;
        workspace.windows.insert(new_window_id, new_window);

        let (source_column_id, source_column_index, source_window_index) = workspace
            .position_for_window(workspace.active_window)
            .ok_or(DomainError::MissingWorkspaceWindow(workspace.active_window))?;

        match direction {
            Direction::Left | Direction::Right => {
                let source_width = workspace
                    .columns
                    .get(&source_column_id)
                    .map(|column| column.width)
                    .expect("active column should exist");
                let (retained_width, new_width) =
                    split_top_level_extent(source_width, MIN_WORKSPACE_WINDOW_WIDTH);
                let column = workspace
                    .columns
                    .get_mut(&source_column_id)
                    .expect("active column should exist");
                column.width = retained_width;

                let mut new_column = WorkspaceColumnRecord::new(new_window_id);
                new_column.width = new_width;
                let insert_index = if matches!(direction, Direction::Left) {
                    source_column_index
                } else {
                    source_column_index + 1
                };
                workspace.insert_column_at(insert_index, new_column);
            }
            Direction::Up | Direction::Down => {
                let source_window_height = workspace
                    .windows
                    .get(&workspace.active_window)
                    .map(|window| window.height)
                    .ok_or(DomainError::MissingWorkspaceWindow(workspace.active_window))?;
                let (retained_height, new_height) =
                    split_top_level_extent(source_window_height, MIN_WORKSPACE_WINDOW_HEIGHT);
                let source_window = workspace
                    .windows
                    .get_mut(&workspace.active_window)
                    .ok_or(DomainError::MissingWorkspaceWindow(workspace.active_window))?;
                source_window.height = retained_height;
                let new_window = workspace
                    .windows
                    .get_mut(&new_window_id)
                    .ok_or(DomainError::MissingWorkspaceWindow(new_window_id))?;
                new_window.height = new_height;

                let column = workspace
                    .columns
                    .get_mut(&source_column_id)
                    .expect("active column should exist");
                let insert_index = if matches!(direction, Direction::Up) {
                    source_window_index
                } else {
                    source_window_index + 1
                };
                column.window_order.insert(insert_index, new_window_id);
                column.active_window = new_window_id;
            }
        }

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

    pub fn acknowledge_pane_notifications(
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
        workspace.acknowledge_pane_notifications(pane_id);
        Ok(())
    }

    pub fn mark_surface_completed(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
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
        let surface = pane
            .surfaces
            .get_mut(&surface_id)
            .ok_or(DomainError::SurfaceNotInPane {
                workspace_id,
                pane_id,
                surface_id,
            })?;

        surface.attention = AttentionState::Completed;
        surface.metadata.agent_active = false;
        surface.metadata.last_signal_at = Some(OffsetDateTime::now_utc());
        workspace.acknowledge_surface_notifications(pane_id, surface_id);
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
        let (column_id, _, _) = workspace
            .position_for_window(active_window)
            .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
        match direction {
            Direction::Left => {
                let column = workspace
                    .columns
                    .get_mut(&column_id)
                    .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
                column.width = (column.width - amount).max(MIN_WORKSPACE_WINDOW_WIDTH);
            }
            Direction::Right => {
                let column = workspace
                    .columns
                    .get_mut(&column_id)
                    .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
                column.width = (column.width + amount).max(MIN_WORKSPACE_WINDOW_WIDTH);
            }
            Direction::Up => {
                let window = workspace
                    .active_window_record_mut()
                    .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
                window.height = (window.height - amount).max(MIN_WORKSPACE_WINDOW_HEIGHT);
            }
            Direction::Down => {
                let window = workspace
                    .active_window_record_mut()
                    .ok_or(DomainError::MissingWorkspaceWindow(active_window))?;
                window.height = (window.height + amount).max(MIN_WORKSPACE_WINDOW_HEIGHT);
            }
        }
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

    pub fn set_workspace_column_width(
        &mut self,
        workspace_id: WorkspaceId,
        workspace_column_id: WorkspaceColumnId,
        width: i32,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let column = workspace
            .columns
            .get_mut(&workspace_column_id)
            .ok_or(DomainError::MissingWorkspaceColumn(workspace_column_id))?;
        column.width = width.max(MIN_WORKSPACE_WINDOW_WIDTH);
        Ok(())
    }

    pub fn set_workspace_window_height(
        &mut self,
        workspace_id: WorkspaceId,
        workspace_window_id: WorkspaceWindowId,
        height: i32,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        let window = workspace
            .windows
            .get_mut(&workspace_window_id)
            .ok_or(DomainError::MissingWorkspaceWindow(workspace_window_id))?;
        window.height = height.max(MIN_WORKSPACE_WINDOW_HEIGHT);
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
        let surface_id = self
            .workspaces
            .values()
            .find_map(|workspace| {
                workspace
                    .panes
                    .get(&pane_id)
                    .map(|pane| pane.active_surface)
            })
            .ok_or(DomainError::MissingPane(pane_id))?;
        self.update_surface_metadata(surface_id, patch)
    }

    pub fn update_surface_metadata(
        &mut self,
        surface_id: SurfaceId,
        patch: PaneMetadataPatch,
    ) -> Result<(), DomainError> {
        let pane = self
            .workspaces
            .values_mut()
            .find_map(|workspace| {
                workspace
                    .panes
                    .values_mut()
                    .find(|pane| pane.surfaces.contains_key(&surface_id))
            })
            .ok_or(DomainError::MissingSurface(surface_id))?;
        let surface = pane
            .surfaces
            .get_mut(&surface_id)
            .ok_or(DomainError::MissingSurface(surface_id))?;

        if patch.title.is_some() {
            surface.metadata.title = patch.title;
        }
        if patch.cwd.is_some() {
            surface.metadata.cwd = patch.cwd;
        }
        if patch.url.is_some() {
            surface.metadata.url = patch.url;
        }
        if patch.repo_name.is_some() {
            surface.metadata.repo_name = patch.repo_name;
        }
        if patch.git_branch.is_some() {
            surface.metadata.git_branch = patch.git_branch;
        }
        if let Some(ports) = patch.ports {
            surface.metadata.ports = ports;
        }
        if patch.agent_kind.is_some() {
            surface.metadata.agent_kind = patch.agent_kind;
        }

        Ok(())
    }

    pub fn apply_signal(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        event: SignalEvent,
    ) -> Result<(), DomainError> {
        let surface_id = self
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .ok_or(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id,
            })?;
        self.apply_surface_signal(workspace_id, pane_id, surface_id, event)
    }

    pub fn apply_surface_signal(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
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
        let surface = pane
            .surfaces
            .get_mut(&surface_id)
            .ok_or(DomainError::SurfaceNotInPane {
                workspace_id,
                pane_id,
                surface_id,
            })?;

        let notification_title = event
            .metadata
            .as_ref()
            .and_then(|metadata| {
                metadata
                    .agent_title
                    .clone()
                    .or_else(|| metadata.title.clone())
            });
        let metadata_reported_inactive = event
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.agent_active)
            .is_some_and(|active| !active);
        let (surface_attention, should_acknowledge_surface_notifications) = {
            let mut acknowledged_inactive_resolution = false;
            if let Some(metadata) = event.metadata {
                surface.metadata.title = metadata.title;
                if metadata.agent_title.is_some() {
                    surface.metadata.agent_title = metadata.agent_title;
                }
                surface.metadata.cwd = metadata.cwd;
                surface.metadata.repo_name = metadata.repo_name;
                surface.metadata.git_branch = metadata.git_branch;
                surface.metadata.ports = metadata.ports;
                surface.metadata.agent_kind = metadata.agent_kind;
                if let Some(agent_active) = metadata.agent_active {
                    surface.metadata.agent_active = agent_active;
                }
            }
            if !matches!(event.kind, SignalKind::Metadata) {
                surface.metadata.last_signal_at = Some(event.timestamp);
                surface.attention = map_signal_to_attention(&event.kind);
                if let Some(agent_active) = signal_agent_active(&event.kind) {
                    surface.metadata.agent_active = agent_active;
                }
            } else if metadata_reported_inactive
                && matches!(
                    surface.attention,
                    AttentionState::Busy | AttentionState::WaitingInput
                )
            {
                surface.attention = AttentionState::Completed;
                surface.metadata.last_signal_at = Some(event.timestamp);
                acknowledged_inactive_resolution = true;
            }

            (surface.attention, acknowledged_inactive_resolution)
        };

        if should_acknowledge_surface_notifications {
            workspace.acknowledge_surface_notifications(pane_id, surface_id);
        }

        if signal_creates_notification(&event.kind)
            && let Some(message) = event.message
        {
            workspace.notifications.push(NotificationItem {
                pane_id,
                surface_id,
                kind: event.kind,
                state: surface_attention,
                title: notification_title,
                message,
                created_at: event.timestamp,
                cleared_at: None,
            });
        }

        Ok(())
    }

    pub fn create_surface(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        kind: PaneKind,
    ) -> Result<SurfaceId, DomainError> {
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
        let surface = SurfaceRecord::new(kind);
        let surface_id = surface.id;
        pane.insert_surface(surface);
        workspace.focus_pane(pane_id);
        Ok(surface_id)
    }

    pub fn focus_surface(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> Result<(), DomainError> {
        let workspace = self
            .workspaces
            .get_mut(&workspace_id)
            .ok_or(DomainError::MissingWorkspace(workspace_id))?;
        if !workspace.focus_surface(pane_id, surface_id) {
            return Err(DomainError::SurfaceNotInPane {
                workspace_id,
                pane_id,
                surface_id,
            });
        }
        Ok(())
    }

    pub fn close_surface(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
    ) -> Result<(), DomainError> {
        let close_entire_pane = self
            .workspaces
            .get(&workspace_id)
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .ok_or(DomainError::PaneNotInWorkspace {
                workspace_id,
                pane_id,
            })?
            .surfaces
            .len()
            <= 1;

        if close_entire_pane {
            return self.close_pane(workspace_id, pane_id);
        }

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
        if pane.surfaces.shift_remove(&surface_id).is_none() {
            return Err(DomainError::SurfaceNotInPane {
                workspace_id,
                pane_id,
                surface_id,
            });
        }
        pane.normalize();
        workspace
            .notifications
            .retain(|item| item.surface_id != surface_id);
        if workspace.active_pane == pane_id {
            workspace.acknowledge_pane_notifications(pane_id);
        }
        Ok(())
    }

    pub fn move_surface(
        &mut self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
        surface_id: SurfaceId,
        to_index: usize,
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
        if !pane.move_surface(surface_id, to_index) {
            return Err(DomainError::SurfaceNotInPane {
                workspace_id,
                pane_id,
                surface_id,
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
        let (column_id, column_index, window_index) = workspace
            .position_for_window(window_id)
            .ok_or(DomainError::MissingWorkspaceWindow(window_id))?;

        let window_leaf_count = workspace
            .windows
            .get(&window_id)
            .map(|window| window.layout.leaves().len())
            .unwrap_or_default();
        if window_leaf_count <= 1 && workspace.windows.len() > 1 {
            let column = workspace
                .columns
                .get_mut(&column_id)
                .expect("window column should exist");
            column.window_order.remove(window_index);
            let same_column_survived = !column.window_order.is_empty();
            if same_column_survived {
                if !column.window_order.contains(&column.active_window) {
                    let replacement_index = window_index.min(column.window_order.len() - 1);
                    column.active_window = column.window_order[replacement_index];
                }
            } else {
                workspace.columns.shift_remove(&column_id);
            }

            workspace.windows.shift_remove(&window_id);
            workspace.panes.shift_remove(&pane_id);
            workspace
                .notifications
                .retain(|item| item.pane_id != pane_id);
            if let Some(next_window_id) = workspace.fallback_window_after_close(
                column_index,
                window_index,
                same_column_survived,
            ) {
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
        let now = OffsetDateTime::now_utc();

        let summaries = window
            .workspace_order
            .iter()
            .filter_map(|workspace_id| self.workspaces.get(workspace_id))
            .map(|workspace| {
                let counts = workspace.attention_counts();
                let agent_summaries = workspace.agent_summaries(now);
                let highest_attention = workspace
                    .panes
                    .values()
                    .map(PaneRecord::highest_attention)
                    .max_by_key(|attention| attention.rank())
                    .unwrap_or(AttentionState::Normal);
                let unread = workspace
                    .notifications
                    .iter()
                    .filter(|notification| notification.cleared_at.is_none())
                    .collect::<Vec<_>>();
                let unread_attention = unread
                    .iter()
                    .map(|notification| notification.state)
                    .max_by_key(|attention| attention.rank());
                let latest_notification = unread
                    .iter()
                    .max_by_key(|notification| notification.created_at)
                    .map(|notification| notification.message.clone());

                WorkspaceSummary {
                    workspace_id: workspace.id,
                    label: workspace.label.clone(),
                    active_pane: workspace.active_pane,
                    repo_hint: workspace.repo_hint().map(str::to_owned),
                    agent_summaries,
                    counts_by_attention: counts,
                    highest_attention,
                    display_attention: unread_attention.unwrap_or(highest_attention),
                    unread_count: unread.len(),
                    latest_notification,
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
                    .filter(|notification| notification.cleared_at.is_none())
                    .map(move |notification| ActivityItem {
                        workspace_id: workspace.id,
                        workspace_window_id: workspace.window_for_pane(notification.pane_id),
                        pane_id: notification.pane_id,
                        surface_id: notification.surface_id,
                        kind: notification.kind.clone(),
                        state: notification.state,
                        title: notification.title.clone(),
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
struct CurrentWorkspaceSerde {
    id: WorkspaceId,
    label: String,
    columns: IndexMap<WorkspaceColumnId, WorkspaceColumnRecord>,
    windows: IndexMap<WorkspaceWindowId, WorkspaceWindowRecord>,
    active_window: WorkspaceWindowId,
    panes: IndexMap<PaneId, PaneRecord>,
    active_pane: PaneId,
    #[serde(default)]
    viewport: WorkspaceViewport,
    #[serde(default)]
    notifications: Vec<NotificationItem>,
    #[serde(default)]
    custom_color: Option<String>,
}

impl CurrentWorkspaceSerde {
    fn into_workspace(self) -> Workspace {
        let mut workspace = Workspace {
            id: self.id,
            label: self.label,
            columns: self.columns,
            windows: self.windows,
            active_window: self.active_window,
            panes: self.panes,
            active_pane: self.active_pane,
            viewport: self.viewport,
            notifications: self.notifications,
            custom_color: self.custom_color,
        };
        workspace.normalize();
        workspace
    }
}

const RECENT_INACTIVE_AGENT_RETENTION: Duration = Duration::minutes(15);

fn signal_creates_notification(kind: &SignalKind) -> bool {
    matches!(
        kind,
        SignalKind::Started
            | SignalKind::Completed
            | SignalKind::WaitingInput
            | SignalKind::Error
            | SignalKind::Notification
    )
}

fn is_agent_kind(agent_kind: Option<&str>) -> bool {
    agent_kind
        .map(str::trim)
        .is_some_and(|agent| !agent.is_empty() && agent != "shell")
}

fn recent_inactive_cutoff(now: OffsetDateTime) -> OffsetDateTime {
    now - RECENT_INACTIVE_AGENT_RETENTION
}

fn workspace_agent_state(
    surface: &SurfaceRecord,
    now: OffsetDateTime,
) -> Option<WorkspaceAgentState> {
    if !is_agent_kind(surface.metadata.agent_kind.as_deref()) {
        return None;
    }

    if !surface.metadata.agent_active {
        return surface
            .metadata
            .last_signal_at
            .filter(|timestamp| *timestamp >= recent_inactive_cutoff(now))
            .map(|_| WorkspaceAgentState::Inactive);
    }

    match surface.attention {
        AttentionState::Busy => Some(WorkspaceAgentState::Working),
        AttentionState::WaitingInput => Some(WorkspaceAgentState::Waiting),
        AttentionState::Completed | AttentionState::Error | AttentionState::Normal => surface
            .metadata
            .last_signal_at
            .filter(|timestamp| *timestamp >= recent_inactive_cutoff(now))
            .map(|_| WorkspaceAgentState::Inactive),
    }
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
        SignalKind::Metadata => AttentionState::Normal,
        SignalKind::Started | SignalKind::Progress => AttentionState::Busy,
        SignalKind::Completed => AttentionState::Completed,
        SignalKind::WaitingInput => AttentionState::WaitingInput,
        SignalKind::Error => AttentionState::Error,
        SignalKind::Notification => AttentionState::WaitingInput,
    }
}

fn signal_agent_active(kind: &SignalKind) -> Option<bool> {
    match kind {
        SignalKind::Metadata => None,
        SignalKind::Started | SignalKind::Progress | SignalKind::WaitingInput => Some(true),
        SignalKind::Completed | SignalKind::Error => Some(false),
        SignalKind::Notification => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::SignalPaneMetadata;

    #[test]
    fn creating_workspace_windows_creates_columns_and_stacks() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let first_window_id = model
            .active_workspace()
            .map(|workspace| workspace.active_window)
            .expect("window");

        let right_pane = model
            .create_workspace_window(workspace_id, Direction::Right)
            .expect("window created");
        let stacked_pane = model
            .create_workspace_window(workspace_id, Direction::Down)
            .expect("stacked window created");
        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let right_column = workspace
            .columns
            .values()
            .find(|column| column.window_order.contains(&workspace.active_window))
            .expect("active column");

        assert_eq!(workspace.windows.len(), 3);
        assert_eq!(workspace.columns.len(), 2);
        assert_eq!(workspace.active_pane, stacked_pane);
        assert_eq!(right_column.width, MIN_WORKSPACE_WINDOW_WIDTH);
        assert_eq!(right_column.window_order.len(), 2);
        assert_ne!(workspace.active_window, first_window_id);
        assert!(
            workspace
                .columns
                .values()
                .any(|column| {
                    column.window_order == vec![first_window_id]
                        && column.width == MIN_WORKSPACE_WINDOW_WIDTH
                })
        );
        let upper_window_id = right_column.window_order[0];
        assert_eq!(
            workspace
                .windows
                .get(&upper_window_id)
                .expect("window")
                .active_pane,
            right_pane
        );
        assert_eq!(
            workspace
                .windows
                .get(&upper_window_id)
                .expect("window")
                .height,
            (DEFAULT_WORKSPACE_WINDOW_HEIGHT + 1) / 2
        );
        assert_eq!(
            workspace
                .windows
                .get(&workspace.active_window)
                .expect("window")
                .height,
            DEFAULT_WORKSPACE_WINDOW_HEIGHT / 2
        );
    }

    #[test]
    fn creating_workspace_window_clamps_split_column_width_to_minimum() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let workspace = model.active_workspace().expect("workspace");
        let column_id = workspace.active_column_id().expect("active column");

        model
            .set_workspace_column_width(workspace_id, column_id, MIN_WORKSPACE_WINDOW_WIDTH + 80)
            .expect("set width");
        model
            .create_workspace_window(workspace_id, Direction::Right)
            .expect("window created");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let widths = workspace
            .columns
            .values()
            .map(|column| column.width)
            .collect::<Vec<_>>();
        assert_eq!(widths, vec![MIN_WORKSPACE_WINDOW_WIDTH, MIN_WORKSPACE_WINDOW_WIDTH]);
    }

    #[test]
    fn creating_workspace_window_clamps_split_window_height_to_minimum() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let window_id = model
            .active_workspace()
            .map(|workspace| workspace.active_window)
            .expect("active window");

        model
            .set_workspace_window_height(workspace_id, window_id, MIN_WORKSPACE_WINDOW_HEIGHT + 50)
            .expect("set height");
        model
            .create_workspace_window(workspace_id, Direction::Down)
            .expect("window created");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let heights = workspace
            .windows
            .values()
            .map(|window| window.height)
            .collect::<Vec<_>>();
        assert_eq!(heights, vec![MIN_WORKSPACE_WINDOW_HEIGHT, MIN_WORKSPACE_WINDOW_HEIGHT]);
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
        let lower_window_pane = model
            .create_workspace_window(workspace_id, Direction::Down)
            .expect("window");
        let lower_right_pane = model
            .split_pane(workspace_id, Some(lower_window_pane), SplitAxis::Vertical)
            .expect("split");

        model
            .focus_pane(workspace_id, lower_window_pane)
            .expect("focus lower window");
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
            lower_window_pane
        );

        model
            .focus_pane(workspace_id, lower_right_pane)
            .expect("focus lower pane");
        model
            .focus_pane_direction(workspace_id, Direction::Up)
            .expect("move up");
        assert_eq!(
            model
                .workspaces
                .get(&workspace_id)
                .expect("workspace")
                .active_pane,
            right_window_pane
        );

        model
            .focus_pane_direction(workspace_id, Direction::Down)
            .expect("move down again");
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
        let lower_window_pane = model
            .create_workspace_window(workspace_id, Direction::Down)
            .expect("window");

        model
            .close_pane(workspace_id, lower_window_pane)
            .expect("close pane");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        assert_eq!(workspace.windows.len(), 2);
        assert!(!workspace.panes.contains_key(&lower_window_pane));
        assert_eq!(workspace.active_pane, right_window_pane);
        let right_column = workspace
            .columns
            .values()
            .find(|column| column.window_order.contains(&workspace.active_window))
            .expect("right column");
        assert_eq!(right_column.window_order.len(), 1);
    }

    #[test]
    fn moving_surface_reorders_pane_without_changing_active_surface() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let first_surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");

        let second_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("second surface");
        let third_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("third surface");

        model
            .focus_surface(workspace_id, pane_id, second_surface_id)
            .expect("focus second surface");
        model
            .move_surface(workspace_id, pane_id, second_surface_id, 0)
            .expect("move second surface to front");

        let pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .expect("pane");
        let order = pane.surface_ids().collect::<Vec<_>>();

        assert_eq!(
            order,
            vec![second_surface_id, first_surface_id, third_surface_id]
        );
        assert_eq!(pane.active_surface, second_surface_id);
    }

    #[test]
    fn moving_surface_clamps_to_end_of_pane() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let first_surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");
        let second_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("second surface");
        let third_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("third surface");

        model
            .move_surface(workspace_id, pane_id, first_surface_id, usize::MAX)
            .expect("move first surface to end");

        let pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .expect("pane");
        let order = pane.surface_ids().collect::<Vec<_>>();

        assert_eq!(
            order,
            vec![second_surface_id, third_surface_id, first_surface_id]
        );
    }

    #[test]
    fn moving_surface_to_current_index_is_a_noop() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let first_surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");
        let second_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("second surface");

        model
            .move_surface(workspace_id, pane_id, second_surface_id, 1)
            .expect("move second surface to current slot");

        let pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .expect("pane");
        let order = pane.surface_ids().collect::<Vec<_>>();

        assert_eq!(order, vec![first_surface_id, second_surface_id]);
        assert_eq!(pane.active_surface, second_surface_id);
    }

    #[test]
    fn closing_surface_after_reorder_removes_the_requested_surface() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let first_surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");
        let second_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("second surface");
        let third_surface_id = model
            .create_surface(workspace_id, pane_id, PaneKind::Terminal)
            .expect("third surface");

        model
            .move_surface(workspace_id, pane_id, first_surface_id, 2)
            .expect("move first surface to end");
        model
            .close_surface(workspace_id, pane_id, second_surface_id)
            .expect("close second surface");

        let pane = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .expect("pane");
        let order = pane.surface_ids().collect::<Vec<_>>();

        assert_eq!(order, vec![third_surface_id, first_surface_id]);
        assert!(!order.contains(&second_surface_id));
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
        model
            .resize_active_window(workspace_id, Direction::Down, 90)
            .expect("resize height");

        let workspace = model.workspaces.get(&workspace_id).expect("workspace");
        let window = workspace.active_window_record().expect("window");
        let column = workspace
            .active_column_id()
            .and_then(|column_id| workspace.columns.get(&column_id))
            .expect("column");
        let LayoutNode::Split { ratio, .. } = &window.layout else {
            panic!("expected split layout");
        };
        assert_eq!(*ratio, 440);
        assert_eq!(column.width, DEFAULT_WORKSPACE_WINDOW_WIDTH + 120);
        assert_eq!(window.height, DEFAULT_WORKSPACE_WINDOW_HEIGHT + 90);
    }

    #[test]
    fn clean_break_rejects_legacy_workspace_layouts() {
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

        let decoded = serde_json::from_value::<PersistedSession>(encoded);
        assert!(decoded.is_err());
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

    #[test]
    fn notification_signal_maps_to_waiting_attention() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::new(
                    "notify:Codex",
                    SignalKind::Notification,
                    Some("Turn complete".into()),
                ),
            )
            .expect("signal applied");

        let surface = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(PaneRecord::active_surface)
            .expect("surface");
        assert_eq!(surface.attention, AttentionState::WaitingInput);
    }

    #[test]
    fn progress_signals_update_attention_without_creating_activity_items() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;

        model
            .update_pane_metadata(
                pane_id,
                PaneMetadataPatch {
                    title: Some("Codex".into()),
                    cwd: None,
                    url: None,
                    repo_name: None,
                    git_branch: None,
                    ports: None,
                    agent_kind: Some("codex".into()),
                },
            )
            .expect("metadata updated");
        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::new("test", SignalKind::Progress, Some("Still working".into())),
            )
            .expect("signal applied");

        let surface = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(PaneRecord::active_surface)
            .expect("surface");

        assert_eq!(surface.attention, AttentionState::Busy);
        assert!(model.activity_items().is_empty());
    }

    #[test]
    fn metadata_signals_do_not_keep_recent_inactive_agents_alive() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let stale_timestamp = OffsetDateTime::now_utc() - Duration::minutes(20);

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent {
                    source: "test".into(),
                    kind: SignalKind::Completed,
                    message: Some("Done".into()),
                    metadata: Some(SignalPaneMetadata {
                        title: None,
                        agent_title: Some("Codex".into()),
                        cwd: None,
                        repo_name: None,
                        git_branch: None,
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(false),
                    }),
                    timestamp: stale_timestamp,
                },
            )
            .expect("completed signal applied");

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::with_metadata(
                    "test",
                    SignalKind::Metadata,
                    None,
                    Some(SignalPaneMetadata {
                        title: Some("codex :: taskers".into()),
                        agent_title: None,
                        cwd: Some("/tmp".into()),
                        repo_name: Some("taskers".into()),
                        git_branch: Some("main".into()),
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(false),
                    }),
                ),
            )
            .expect("metadata signal applied");

        let summaries = model
            .workspace_summaries(model.active_window)
            .expect("workspace summaries");
        assert!(
            summaries
                .first()
                .expect("summary")
                .agent_summaries
                .is_empty()
        );

        let surface = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(PaneRecord::active_surface)
            .expect("surface");
        assert_eq!(surface.metadata.last_signal_at, Some(stale_timestamp));
    }

    #[test]
    fn marking_surface_completed_clears_activity_and_keeps_recent_inactive_status() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::with_metadata(
                    "test",
                    SignalKind::WaitingInput,
                    Some("Need review".into()),
                    Some(SignalPaneMetadata {
                        title: None,
                        agent_title: Some("Codex".into()),
                        cwd: None,
                        repo_name: None,
                        git_branch: None,
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(true),
                    }),
                ),
            )
            .expect("waiting signal applied");

        assert_eq!(model.activity_items().len(), 1);

        model
            .mark_surface_completed(workspace_id, pane_id, surface_id)
            .expect("mark completed");

        let surface = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .and_then(PaneRecord::active_surface)
            .expect("surface");
        assert_eq!(surface.attention, AttentionState::Completed);
        assert!(model.activity_items().is_empty());

        let summaries = model
            .workspace_summaries(model.active_window)
            .expect("workspace summaries");
        assert_eq!(
            summaries
                .first()
                .and_then(|summary| summary.agent_summaries.first())
                .map(|summary| summary.state),
            Some(WorkspaceAgentState::Inactive)
        );
    }

    #[test]
    fn metadata_inactive_resolves_waiting_agent_state() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let pane_id = model.active_workspace().expect("workspace").active_pane;

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::with_metadata(
                    "test",
                    SignalKind::WaitingInput,
                    Some("Need input".into()),
                    Some(SignalPaneMetadata {
                        title: None,
                        agent_title: Some("Codex".into()),
                        cwd: None,
                        repo_name: None,
                        git_branch: None,
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(true),
                    }),
                ),
            )
            .expect("waiting signal applied");

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::with_metadata(
                    "test",
                    SignalKind::Metadata,
                    None,
                    Some(SignalPaneMetadata {
                        title: Some("codex :: taskers".into()),
                        agent_title: None,
                        cwd: Some("/tmp".into()),
                        repo_name: Some("taskers".into()),
                        git_branch: Some("main".into()),
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(false),
                    }),
                ),
            )
            .expect("metadata signal applied");

        let workspace = model.active_workspace().expect("workspace");
        let surface = workspace
            .panes
            .get(&pane_id)
            .and_then(PaneRecord::active_surface)
            .expect("surface");
        assert_eq!(surface.attention, AttentionState::Completed);
        assert!(!surface.metadata.agent_active);
        assert!(
            workspace
                .notifications
                .iter()
                .all(|item| item.cleared_at.is_some())
        );

        let summaries = model
            .workspace_summaries(model.active_window)
            .expect("workspace summaries");
        assert_eq!(
            summaries
                .first()
                .and_then(|summary| summary.agent_summaries.first())
                .map(|summary| summary.state),
            Some(WorkspaceAgentState::Inactive)
        );
    }

    #[test]
    fn focusing_waiting_agent_does_not_clear_attention_item() {
        let mut model = AppModel::new("Main");
        let workspace_id = model.active_workspace_id().expect("workspace");
        let window_id = model.active_window;
        let pane_id = model.active_workspace().expect("workspace").active_pane;
        let surface_id = model
            .active_workspace()
            .and_then(|workspace| workspace.panes.get(&pane_id))
            .map(|pane| pane.active_surface)
            .expect("surface id");

        model
            .apply_signal(
                workspace_id,
                pane_id,
                SignalEvent::with_metadata(
                    "test",
                    SignalKind::WaitingInput,
                    Some("Need review".into()),
                    Some(SignalPaneMetadata {
                        title: None,
                        agent_title: Some("Codex".into()),
                        cwd: None,
                        repo_name: None,
                        git_branch: None,
                        ports: Vec::new(),
                        agent_kind: Some("codex".into()),
                        agent_active: Some(true),
                    }),
                ),
            )
            .expect("waiting signal applied");

        let other_workspace_id = model.create_workspace("Docs");
        assert_eq!(model.activity_items().len(), 1);

        model
            .switch_workspace(window_id, workspace_id)
            .expect("switch back to waiting workspace");
        model
            .focus_surface(workspace_id, pane_id, surface_id)
            .expect("focus waiting surface");

        let activity_items = model.activity_items();
        assert_eq!(activity_items.len(), 1);
        assert_eq!(activity_items[0].state, AttentionState::WaitingInput);
        assert_eq!(
            model
                .workspaces
                .get(&workspace_id)
                .expect("workspace")
                .notifications
                .iter()
                .filter(|item| item.cleared_at.is_none())
                .count(),
            1
        );
        assert_eq!(model.active_workspace_id(), Some(workspace_id));
        assert_ne!(workspace_id, other_workspace_id);
    }
}
