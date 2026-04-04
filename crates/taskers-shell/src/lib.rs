mod icons;
mod theme;

use dioxus::html::{
    PointerData,
    input_data::MouseButton,
    point_interaction::{InteractionLocation, PointerInteraction},
};
use dioxus::prelude::*;
use taskers_core::{
    ActivityItemSnapshot, AgentSessionSnapshot, AttentionRingState, AttentionState,
    BrowserChromeSnapshot, Direction, DragSessionSnapshot, LayoutNodeSnapshot, LivePaneSnapshot,
    NotificationPreferenceKey, PaneContainerId, PaneId, PaneKind, PaneSnapshot,
    PaneTabDragSessionSnapshot, PaneTabId, PaneTabLayoutSnapshot, PaneTabSnapshot,
    ProgressSnapshot, PullRequestSnapshot, RuntimeIdentitySnapshot, RuntimeStateSnapshot,
    RuntimeStatus, SettingsSnapshot, SharedCore, ShellAction, ShellSection, ShellSnapshot,
    ShortcutAction, ShortcutBindingSnapshot, SplitAxis, SurfaceDragSessionSnapshot, SurfaceId,
    SurfaceKind, SurfaceSnapshot, VcsCommand, VcsFileEntry, VcsFileStatus, VcsMode,
    VcsPanelSnapshot, VcsSnapshot, WindowTabDragSessionSnapshot, WorkspaceId,
    WorkspaceLogEntrySnapshot, WorkspaceSummary, WorkspaceViewSnapshot, WorkspaceWindowMoveTarget,
    WorkspaceWindowSnapshot, WorkspaceWindowTabId, WorkspaceWindowTabSnapshot,
};
use taskers_shell_core as taskers_core;

type DraggedSurface = SurfaceDragSessionSnapshot;
type DraggedWindowTab = WindowTabDragSessionSnapshot;
type DraggedPaneTab = PaneTabDragSessionSnapshot;

const WORKSPACE_DRAG_MIME: &str = "application/x-taskers-workspace";
const WINDOW_DRAG_MIME: &str = "application/x-taskers-window";
const SURFACE_DRAG_THRESHOLD_PX: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
struct DraggedWindow {
    window_id: taskers_core::WorkspaceWindowId,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WindowTabDropTarget {
    BeforeTab {
        window_id: taskers_core::WorkspaceWindowId,
        tab_id: WorkspaceWindowTabId,
    },
    AppendToWindow {
        window_id: taskers_core::WorkspaceWindowId,
    },
}

#[derive(Clone, Copy, PartialEq)]
struct WindowTabDragCandidate {
    window_id: taskers_core::WorkspaceWindowId,
    tab_id: WorkspaceWindowTabId,
    start_x: f64,
    start_y: f64,
}

#[derive(Clone, Copy, PartialEq)]
struct PaneTabDragCandidate {
    pane_container_id: PaneContainerId,
    pane_tab_id: PaneTabId,
    start_x: f64,
    start_y: f64,
}

#[derive(Clone, Copy, PartialEq)]
struct SurfaceDragCandidate {
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    surface_id: SurfaceId,
    start_x: f64,
    start_y: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaneTabDropTarget {
    BeforeTab {
        pane_container_id: PaneContainerId,
        pane_tab_id: PaneTabId,
    },
    AppendToContainer {
        pane_container_id: PaneContainerId,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SurfaceDropTarget {
    AppendToPane {
        pane_id: PaneId,
    },
    BeforeSurface {
        pane_id: PaneId,
        surface_id: SurfaceId,
    },
    SplitPane {
        pane_id: PaneId,
        direction: Direction,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragPreviewKind {
    WindowTab,
    PaneTab,
    Surface,
}

#[derive(Clone, PartialEq)]
struct DragPreviewInfo {
    kind: DragPreviewKind,
    runtime_key: String,
    runtime_state: RuntimeStateSnapshot,
    title: String,
    runtime_badge: Option<String>,
}

fn runtime_state_class(state: RuntimeStateSnapshot) -> &'static str {
    match state {
        RuntimeStateSnapshot::Idle => "runtime-state-idle",
        RuntimeStateSnapshot::Working => "runtime-state-working",
        RuntimeStateSnapshot::Waiting => "runtime-state-waiting",
        RuntimeStateSnapshot::Completed => "runtime-state-completed",
        RuntimeStateSnapshot::Failed => "runtime-state-failed",
    }
}

fn attention_ring_class(state: Option<AttentionRingState>, prefix: &str) -> String {
    state
        .map(|state| format!(" {prefix}-{}", state.slug()))
        .unwrap_or_default()
}

fn render_runtime_icon(runtime: &RuntimeIdentitySnapshot, size: u32, class: &str) -> Element {
    render_runtime_icon_by_key(runtime.key.as_str(), size, class)
}

fn render_runtime_icon_by_key(key: &str, size: u32, class: &str) -> Element {
    match key {
        "codex" => icons::codex(size, class),
        "claude" => icons::claude(size, class),
        "opencode" => icons::opencode(size, class),
        "aider" => icons::aider(size, class),
        "browser" => icons::globe(size, class),
        _ => icons::terminal(size, class),
    }
}

fn surface_primary_label(surface: &SurfaceSnapshot) -> &str {
    surface
        .activity_label
        .as_deref()
        .unwrap_or(surface.title.as_str())
}

fn surface_status_text(surface: &SurfaceSnapshot) -> Option<&str> {
    surface.status_label.as_deref()
}

fn surface_runtime_badge_text(surface: &SurfaceSnapshot) -> Option<&str> {
    if matches!(surface.runtime.key.as_str(), "terminal" | "browser") {
        return None;
    }

    let primary = surface_primary_label(surface).to_ascii_lowercase();
    let runtime = surface.runtime.label.to_ascii_lowercase();
    if primary.contains(&runtime) {
        None
    } else {
        Some(surface.runtime.label.as_str())
    }
}

fn push_surface_summary_part(parts: &mut Vec<String>, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    if parts.iter().all(|existing| existing != value) {
        parts.push(value.to_string());
    }
}

fn surface_summary_title(surface: &SurfaceSnapshot) -> String {
    let mut parts = Vec::new();
    push_surface_summary_part(&mut parts, Some(surface.runtime.label.as_str()));
    push_surface_summary_part(&mut parts, surface.status_label.as_deref());
    push_surface_summary_part(&mut parts, surface.activity_label.as_deref());
    push_surface_summary_part(&mut parts, Some(surface.title.as_str()));
    parts.join(" · ")
}

fn compute_surface_drop_index(
    dragged: DraggedSurface,
    target_pane_id: PaneId,
    ordered_surface_ids: &[SurfaceId],
    before_surface_id: Option<SurfaceId>,
) -> usize {
    let Some(before_surface_id) = before_surface_id else {
        return usize::MAX;
    };

    let Some(mut target_index) = ordered_surface_ids
        .iter()
        .position(|surface_id| *surface_id == before_surface_id)
    else {
        return usize::MAX;
    };

    if dragged.pane_id == target_pane_id
        && let Some(source_index) = ordered_surface_ids
            .iter()
            .position(|surface_id| *surface_id == dragged.surface_id)
        && source_index < target_index
    {
        target_index = target_index.saturating_sub(1);
    }

    target_index
}

fn compute_window_tab_drop_index(
    dragged: DraggedWindowTab,
    target_window_id: taskers_core::WorkspaceWindowId,
    ordered_tab_ids: &[WorkspaceWindowTabId],
    before_tab_id: Option<WorkspaceWindowTabId>,
) -> usize {
    let Some(before_tab_id) = before_tab_id else {
        return usize::MAX;
    };

    let Some(mut target_index) = ordered_tab_ids
        .iter()
        .position(|tab_id| *tab_id == before_tab_id)
    else {
        return usize::MAX;
    };

    if dragged.window_id == target_window_id
        && let Some(source_index) = ordered_tab_ids
            .iter()
            .position(|tab_id| *tab_id == dragged.tab_id)
        && source_index < target_index
    {
        target_index = target_index.saturating_sub(1);
    }

    target_index
}

fn compute_pane_tab_drop_index(
    dragged: DraggedPaneTab,
    target_pane_container_id: PaneContainerId,
    ordered_tab_ids: &[PaneTabId],
    before_tab_id: Option<PaneTabId>,
) -> usize {
    let Some(before_tab_id) = before_tab_id else {
        return usize::MAX;
    };

    let Some(mut target_index) = ordered_tab_ids
        .iter()
        .position(|tab_id| *tab_id == before_tab_id)
    else {
        return usize::MAX;
    };

    if dragged.pane_container_id == target_pane_container_id
        && let Some(source_index) = ordered_tab_ids
            .iter()
            .position(|tab_id| *tab_id == dragged.pane_tab_id)
        && source_index < target_index
    {
        target_index = target_index.saturating_sub(1);
    }

    target_index
}

fn pane_allows_surface_split(
    dragged: Option<DraggedSurface>,
    pane_id: PaneId,
    surface_count: usize,
) -> bool {
    dragged.is_some_and(|dragged| dragged.pane_id != pane_id || surface_count > 1)
}

fn show_surface_backdrop(
    surface_kind: SurfaceKind,
    overview_mode: bool,
    render_live_surfaces_in_overview: bool,
    resize_preview_active: bool,
) -> bool {
    if resize_preview_active {
        return true;
    }

    if overview_mode {
        return !render_live_surfaces_in_overview;
    }

    !matches!(surface_kind, SurfaceKind::Browser)
}

fn prime_drag_transfer(event: &Event<DragData>, mime: &str, payload: &str) {
    let transfer = event.data().data_transfer();
    let _ = transfer.set_data(mime, payload);
    let _ = transfer.set_data("text/plain", payload);
    transfer.set_effect_allowed("move");
    transfer.set_drop_effect("move");
}

fn mark_move_drop(event: &Event<DragData>) {
    event.prevent_default();
    event.data().data_transfer().set_drop_effect("move");
}

fn pointer_client_position(event: &Event<PointerData>) -> (f64, f64) {
    let position = event.data().client_coordinates();
    (position.x, position.y)
}

fn primary_pointer_drag_origin(event: &Event<PointerData>) -> Option<(f64, f64)> {
    if event.data().trigger_button() != Some(MouseButton::Primary) {
        return None;
    }
    let (start_x, start_y) = pointer_client_position(event);
    event.prevent_default();
    event.stop_propagation();
    Some((start_x, start_y))
}

fn window_tab_drag_candidate_from_event(
    event: &Event<PointerData>,
    window_id: taskers_core::WorkspaceWindowId,
    tab_id: WorkspaceWindowTabId,
) -> Option<WindowTabDragCandidate> {
    let (start_x, start_y) = primary_pointer_drag_origin(event)?;
    Some(WindowTabDragCandidate {
        window_id,
        tab_id,
        start_x,
        start_y,
    })
}

fn pane_tab_drag_candidate_from_event(
    event: &Event<PointerData>,
    pane_container_id: PaneContainerId,
    pane_tab_id: PaneTabId,
) -> Option<PaneTabDragCandidate> {
    let (start_x, start_y) = primary_pointer_drag_origin(event)?;
    Some(PaneTabDragCandidate {
        pane_container_id,
        pane_tab_id,
        start_x,
        start_y,
    })
}

fn surface_drag_candidate_from_event(
    event: &Event<PointerData>,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    surface_id: SurfaceId,
) -> Option<SurfaceDragCandidate> {
    let (start_x, start_y) = primary_pointer_drag_origin(event)?;
    Some(SurfaceDragCandidate {
        workspace_id,
        pane_id,
        surface_id,
        start_x,
        start_y,
    })
}

fn drag_threshold_reached(start_x: f64, start_y: f64, current_x: f64, current_y: f64) -> bool {
    let dx = current_x - start_x;
    let dy = current_y - start_y;
    dx.hypot(dy) >= SURFACE_DRAG_THRESHOLD_PX
}

fn surface_drag_threshold_reached(
    candidate: SurfaceDragCandidate,
    current_x: f64,
    current_y: f64,
) -> bool {
    drag_threshold_reached(candidate.start_x, candidate.start_y, current_x, current_y)
}

fn current_dragged_window_tab(snapshot: &ShellSnapshot) -> Option<DraggedWindowTab> {
    match snapshot.drag_session {
        Some(DragSessionSnapshot::WindowTab(dragged)) => Some(dragged),
        _ => None,
    }
}

fn current_dragged_pane_tab(snapshot: &ShellSnapshot) -> Option<DraggedPaneTab> {
    match snapshot.drag_session {
        Some(DragSessionSnapshot::PaneTab(dragged)) => Some(dragged),
        _ => None,
    }
}

fn find_workspace_window_tab_snapshot<'a>(
    workspace: &'a WorkspaceViewSnapshot,
    window_id: taskers_core::WorkspaceWindowId,
    tab_id: WorkspaceWindowTabId,
) -> Option<&'a WorkspaceWindowTabSnapshot> {
    workspace
        .columns
        .iter()
        .flat_map(|column| column.windows.iter())
        .find(|window| window.id == window_id)
        .and_then(|window| window.tabs.iter().find(|tab| tab.id == tab_id))
}

fn find_pane_tab_snapshot<'a>(
    node: &'a LayoutNodeSnapshot,
    pane_container_id: PaneContainerId,
    pane_tab_id: PaneTabId,
) -> Option<&'a PaneTabSnapshot> {
    match node {
        LayoutNodeSnapshot::Pane(pane) => {
            if pane.pane_container_id == pane_container_id {
                pane.pane_tabs.iter().find(|tab| tab.id == pane_tab_id)
            } else {
                None
            }
        }
        LayoutNodeSnapshot::Split { first, second, .. } => {
            find_pane_tab_snapshot(first, pane_container_id, pane_tab_id)
                .or_else(|| find_pane_tab_snapshot(second, pane_container_id, pane_tab_id))
        }
    }
}

fn find_surface_snapshot<'a>(
    node: &'a LayoutNodeSnapshot,
    pane_id: PaneId,
    surface_id: SurfaceId,
) -> Option<&'a SurfaceSnapshot> {
    match node {
        LayoutNodeSnapshot::Pane(pane) => {
            find_surface_snapshot_in_pane_tab_layout(&pane.layout, pane_id, surface_id)
        }
        LayoutNodeSnapshot::Split { first, second, .. } => {
            find_surface_snapshot(first, pane_id, surface_id)
                .or_else(|| find_surface_snapshot(second, pane_id, surface_id))
        }
    }
}

fn find_surface_snapshot_in_pane_tab_layout<'a>(
    node: &'a PaneTabLayoutSnapshot,
    pane_id: PaneId,
    surface_id: SurfaceId,
) -> Option<&'a SurfaceSnapshot> {
    match node {
        PaneTabLayoutSnapshot::Pane(pane) => {
            if pane.id == pane_id {
                pane.surfaces
                    .iter()
                    .find(|surface| surface.id == surface_id)
            } else {
                None
            }
        }
        PaneTabLayoutSnapshot::Split { first, second, .. } => {
            find_surface_snapshot_in_pane_tab_layout(first, pane_id, surface_id)
                .or_else(|| find_surface_snapshot_in_pane_tab_layout(second, pane_id, surface_id))
        }
    }
}

fn drag_preview_info(snapshot: &ShellSnapshot) -> Option<DragPreviewInfo> {
    match snapshot.drag_session? {
        DragSessionSnapshot::WindowTab(dragged) => {
            let tab = find_workspace_window_tab_snapshot(
                &snapshot.current_workspace,
                dragged.window_id,
                dragged.tab_id,
            )?;
            Some(DragPreviewInfo {
                kind: DragPreviewKind::WindowTab,
                runtime_key: tab.runtime.key.clone(),
                runtime_state: tab.runtime.state,
                title: tab.title.clone(),
                runtime_badge: None,
            })
        }
        DragSessionSnapshot::PaneTab(dragged) => {
            let tab = find_pane_tab_snapshot(
                &snapshot.current_workspace.layout,
                dragged.pane_container_id,
                dragged.pane_tab_id,
            )?;
            Some(DragPreviewInfo {
                kind: DragPreviewKind::PaneTab,
                runtime_key: tab.runtime.key.clone(),
                runtime_state: tab.runtime.state,
                title: tab.title.clone(),
                runtime_badge: None,
            })
        }
        DragSessionSnapshot::Surface(dragged) => {
            let surface = find_surface_snapshot(
                &snapshot.current_workspace.layout,
                dragged.pane_id,
                dragged.surface_id,
            );
            Some(DragPreviewInfo {
                kind: DragPreviewKind::Surface,
                runtime_key: surface
                    .map(|surface| surface.runtime.key.clone())
                    .unwrap_or_else(|| "terminal".into()),
                runtime_state: surface
                    .map(|surface| surface.runtime.state)
                    .unwrap_or(RuntimeStateSnapshot::Idle),
                title: surface
                    .map(surface_summary_title)
                    .unwrap_or_else(|| "Moving tab".into()),
                runtime_badge: surface
                    .and_then(surface_runtime_badge_text)
                    .map(str::to_string),
            })
        }
    }
}

fn render_drag_preview(snapshot: &ShellSnapshot, pointer: (f64, f64)) -> Element {
    let Some(preview) = drag_preview_info(snapshot) else {
        return rsx! {};
    };
    let style = format!(
        "transform: translate({:.1}px, {:.1}px);",
        pointer.0 + 14.0,
        pointer.1 + 12.0
    );
    let icon_class = format!(
        "drag-preview-icon {}",
        runtime_state_class(preview.runtime_state)
    );
    let card_class = match preview.kind {
        DragPreviewKind::WindowTab => {
            "workspace-window-tab workspace-window-tab-active drag-preview-card drag-preview-window-tab"
        }
        DragPreviewKind::PaneTab | DragPreviewKind::Surface => {
            "surface-tab surface-tab-active drag-preview-card drag-preview-surface-tab"
        }
    };

    rsx! {
        div { class: "drag-preview-shell", style: "{style}",
            div { class: "{card_class}",
                {render_runtime_icon_by_key(preview.runtime_key.as_str(), 11, &icon_class)}
                span { class: "drag-preview-copy",
                    span { class: "drag-preview-title", "{preview.title}" }
                    if let Some(runtime_badge) = &preview.runtime_badge {
                        span { class: "surface-tab-runtime-badge", "{runtime_badge}" }
                    }
                }
            }
        }
    }
}

fn apply_surface_drop(
    core: &SharedCore,
    dragged: DraggedSurface,
    target: SurfaceDropTarget,
    ordered_surface_ids: &[SurfaceId],
) {
    match target {
        SurfaceDropTarget::AppendToPane { pane_id } => {
            core.dispatch_shell_action(ShellAction::MoveSurface {
                surface_id: dragged.surface_id,
                target_pane_id: pane_id,
                target_index: usize::MAX,
            });
        }
        SurfaceDropTarget::BeforeSurface {
            pane_id,
            surface_id,
        } => {
            core.dispatch_shell_action(ShellAction::MoveSurface {
                surface_id: dragged.surface_id,
                target_pane_id: pane_id,
                target_index: compute_surface_drop_index(
                    dragged,
                    pane_id,
                    ordered_surface_ids,
                    Some(surface_id),
                ),
            });
        }
        SurfaceDropTarget::SplitPane { pane_id, direction } => {
            core.dispatch_shell_action(ShellAction::MoveSurfaceToSplit {
                source_pane_id: dragged.pane_id,
                surface_id: dragged.surface_id,
                target_pane_id: pane_id,
                direction,
            });
        }
    }
}

fn app_css(snapshot: &ShellSnapshot) -> String {
    theme::generate_css(
        &theme::resolve_palette(&snapshot.settings.selected_theme_id),
        snapshot.metrics,
        snapshot.attention_panel_visible || snapshot.vcs_panel.visible,
    )
}

#[component]
pub fn TaskersShell(core: SharedCore) -> Element {
    use_context_provider(move || core.clone());

    let core = consume_context::<SharedCore>();
    let revision = use_signal(|| core.revision());

    {
        let core = core.clone();
        let mut revision = revision;
        use_hook(move || {
            let mut revisions = core.subscribe_revisions();
            spawn(async move {
                while revisions.changed().await.is_ok() {
                    revision.set(*revisions.borrow());
                }
            });
        });
    }

    let _ = revision();
    let snapshot = core.snapshot();
    let unread_activity = snapshot.activity.iter().filter(|item| item.unread).count();
    let stylesheet = app_css(&snapshot);
    let toggle_settings = {
        let core = core.clone();
        let section = snapshot.section;
        move |_| {
            let target = if matches!(section, ShellSection::Settings) {
                ShellSection::Workspace
            } else {
                ShellSection::Settings
            };
            core.dispatch_shell_action(ShellAction::ShowSection { section: target });
        }
    };
    let create_workspace = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::CreateWorkspace)
    };
    let jump_unread = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::FocusLatestUnread)
    };
    let toggle_vcs_panel = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ToggleVcsPanel)
    };
    let drag_source = use_signal(|| None::<WorkspaceId>);
    let drag_target = use_signal(|| None::<WorkspaceId>);
    let mut surface_drop_target = use_signal(|| None::<SurfaceDropTarget>);
    let mut surface_drag_candidate = use_signal(|| None::<SurfaceDragCandidate>);
    let mut window_tab_drag_candidate = use_signal(|| None::<WindowTabDragCandidate>);
    let mut pane_tab_drag_candidate = use_signal(|| None::<PaneTabDragCandidate>);
    let mut drag_pointer = use_signal(|| None::<(f64, f64)>);
    let window_drag_source = use_signal(|| None::<DraggedWindow>);
    let window_drop_target = use_signal(|| None::<WorkspaceWindowMoveTarget>);
    let mut window_tab_drop_target = use_signal(|| None::<WindowTabDropTarget>);
    let mut pane_tab_drop_target = use_signal(|| None::<PaneTabDropTarget>);
    let workspace_ids: Vec<WorkspaceId> = snapshot.workspaces.iter().map(|ws| ws.id).collect();
    let dragged_surface = snapshot.surface_drag;
    let dragged_window_tab = current_dragged_window_tab(&snapshot);
    let dragged_pane_tab = current_dragged_pane_tab(&snapshot);
    let track_pointer_drag = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if event.data().held_buttons().contains(MouseButton::Primary) {
                let (current_x, current_y) = pointer_client_position(&event);
                drag_pointer.set(Some((current_x, current_y)));
                let window_candidate = *window_tab_drag_candidate.read();
                let pane_candidate = *pane_tab_drag_candidate.read();
                let surface_candidate = *surface_drag_candidate.read();
                if let Some(candidate) = window_candidate {
                    if drag_threshold_reached(
                        candidate.start_x,
                        candidate.start_y,
                        current_x,
                        current_y,
                    ) {
                        window_tab_drag_candidate.set(None);
                        window_tab_drop_target.set(None);
                        core.dispatch_shell_action(ShellAction::BeginWindowTabDrag {
                            window_id: candidate.window_id,
                            tab_id: candidate.tab_id,
                        });
                        event.stop_propagation();
                    }
                } else if let Some(candidate) = pane_candidate {
                    if drag_threshold_reached(
                        candidate.start_x,
                        candidate.start_y,
                        current_x,
                        current_y,
                    ) {
                        pane_tab_drag_candidate.set(None);
                        pane_tab_drop_target.set(None);
                        core.dispatch_shell_action(ShellAction::BeginPaneTabDrag {
                            pane_container_id: candidate.pane_container_id,
                            pane_tab_id: candidate.pane_tab_id,
                        });
                        event.stop_propagation();
                    }
                } else if let Some(candidate) = surface_candidate
                    && surface_drag_threshold_reached(candidate, current_x, current_y)
                {
                    surface_drag_candidate.set(None);
                    surface_drop_target.set(None);
                    core.dispatch_shell_action(ShellAction::BeginSurfaceDrag {
                        workspace_id: candidate.workspace_id,
                        pane_id: candidate.pane_id,
                        surface_id: candidate.surface_id,
                    });
                    event.stop_propagation();
                }
            }

            if core.snapshot().drag_session.is_some()
                && !event.data().held_buttons().contains(MouseButton::Primary)
            {
                drag_pointer.set(None);
                window_tab_drag_candidate.set(None);
                pane_tab_drag_candidate.set(None);
                surface_drag_candidate.set(None);
                window_tab_drop_target.set(None);
                pane_tab_drop_target.set(None);
                surface_drop_target.set(None);
                if core.snapshot().surface_drag.is_some() {
                    core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
                } else {
                    core.dispatch_shell_action(ShellAction::EndDrag);
                }
                event.stop_propagation();
            }
        }
    };
    let finish_pointer_drag_up = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if window_tab_drag_candidate.read().is_some() {
                window_tab_drag_candidate.set(None);
            }
            if pane_tab_drag_candidate.read().is_some() {
                pane_tab_drag_candidate.set(None);
            }
            if surface_drag_candidate.read().is_some() {
                surface_drag_candidate.set(None);
            }
            drag_pointer.set(None);
            if core.snapshot().drag_session.is_some() {
                window_tab_drop_target.set(None);
                pane_tab_drop_target.set(None);
                surface_drop_target.set(None);
                if core.snapshot().surface_drag.is_some() {
                    core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
                } else {
                    core.dispatch_shell_action(ShellAction::EndDrag);
                }
                event.stop_propagation();
            }
        }
    };
    let finish_pointer_drag_cancel = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if window_tab_drag_candidate.read().is_some() {
                window_tab_drag_candidate.set(None);
            }
            if pane_tab_drag_candidate.read().is_some() {
                pane_tab_drag_candidate.set(None);
            }
            if surface_drag_candidate.read().is_some() {
                surface_drag_candidate.set(None);
            }
            drag_pointer.set(None);
            if core.snapshot().drag_session.is_some() {
                window_tab_drop_target.set(None);
                pane_tab_drop_target.set(None);
                surface_drop_target.set(None);
                if core.snapshot().surface_drag.is_some() {
                    core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
                } else {
                    core.dispatch_shell_action(ShellAction::EndDrag);
                }
                event.stop_propagation();
            }
        }
    };

    let main_class = match snapshot.section {
        ShellSection::Workspace => {
            if snapshot.overview_mode {
                "workspace-main workspace-main-overview"
            } else {
                "workspace-main"
            }
        }
        ShellSection::Settings => "workspace-main workspace-main-settings",
    };

    rsx! {
        style { "{stylesheet}" }
        div {
            class: "app-shell",
            onpointermove: track_pointer_drag,
            onpointerup: finish_pointer_drag_up,
            onpointercancel: finish_pointer_drag_cancel,
            aside { class: "workspace-sidebar",
                div { class: "sidebar-top",
                    button { class: "workspace-add", onclick: create_workspace,
                        {icons::plus(14, "workspace-add-icon")}
                    }
                }
                div { class: "workspace-list",
                    for workspace in &snapshot.workspaces {
                        {render_workspace_item(
                            workspace,
                            core.clone(),
                            drag_source,
                            drag_target,
                            dragged_surface,
                            surface_drop_target,
                            &workspace_ids,
                        )}
                    }
                }
                div { class: "sidebar-footer",
                    button {
                        class: if matches!(snapshot.section, ShellSection::Settings) { "sidebar-settings-btn sidebar-settings-btn-active" } else { "sidebar-settings-btn" },
                        onclick: toggle_settings,
                        {icons::settings(16, "sidebar-settings-icon")}
                    }
                }
            }

            main { class: "{main_class}",
                header { class: "workspace-header",
                    div { class: "workspace-header-main",
                        span { class: "workspace-header-label",
                            if matches!(snapshot.section, ShellSection::Workspace) {
                                "{snapshot.current_workspace.title}"
                            } else {
                                "Settings"
                            }
                        }
                    }
                    if matches!(snapshot.section, ShellSection::Workspace) {
                        div { class: "workspace-header-actions",
                            button {
                                class: if snapshot.vcs_panel.visible {
                                    "pane-action workspace-header-action workspace-header-action-active"
                                } else {
                                    "pane-action workspace-header-action"
                                },
                                r#type: "button",
                                onclick: toggle_vcs_panel,
                                title: "Toggle VCS panel",
                                {icons::git_branch(14, "workspace-header-action-icon")}
                                span { "VCS" }
                            }
                        }
                    }
                }

                if matches!(snapshot.section, ShellSection::Workspace) {
                    div { class: if snapshot.overview_mode { "workspace-canvas workspace-canvas-overview" } else { "workspace-canvas" },
                        {render_workspace_strip(
                            &snapshot.current_workspace,
                            snapshot.overview_mode,
                            snapshot.browser_chrome.as_ref(),
                            core.clone(),
                            &snapshot.runtime_status,
                            surface_drop_target,
                            surface_drag_candidate,
                            dragged_surface,
                            window_drag_source,
                            window_drop_target,
                            window_tab_drag_candidate,
                            dragged_window_tab,
                            window_tab_drop_target,
                            pane_tab_drag_candidate,
                            dragged_pane_tab,
                            pane_tab_drop_target,
                        )}
                    }
                } else {
                    div { class: "settings-canvas",
                        {render_settings(&snapshot.settings, core.clone())}
                    }
                }
            }

            if snapshot.vcs_panel.visible {
                VcsPanel {
                    panel: snapshot.vcs_panel.clone(),
                    core: core.clone(),
                }
            } else if snapshot.attention_panel_visible {
                aside { class: "attention-panel",
                    div { class: "notification-header",
                        div { class: "sidebar-heading", "Notifications" }
                        div { class: "notification-counts",
                            if !snapshot.agents.is_empty() {
                                span { class: "notification-count-pill notification-count-agents",
                                    "{snapshot.agents.len()} agents"
                                }
                            }
                            if unread_activity > 0 {
                                span { class: "notification-count-pill notification-count-unread",
                                    "{unread_activity} unread"
                                }
                                button {
                                    class: "notification-jump-button",
                                    onclick: jump_unread,
                                    "Jump unread"
                                }
                            }
                        }
                    }
                    if snapshot.current_workspace_status.is_some()
                        || snapshot.current_workspace_progress.is_some()
                    {
                        section { class: "attention-section attention-status",
                            div { class: "attention-section-title", "Workspace status" }
                            if let Some(status) = &snapshot.current_workspace_status {
                                div { class: "attention-status-text", "{status}" }
                            }
                            {render_workspace_progress(&snapshot.current_workspace_progress)}
                        }
                    }
                    if !snapshot.agents.is_empty() {
                        div { class: "agent-session-list",
                            for agent in &snapshot.agents {
                                {render_agent_item(agent, core.clone(), &snapshot.current_workspace)}
                            }
                        }
                    }
                    div { class: "notification-timeline",
                        if snapshot.activity.is_empty() {
                            div { class: "notification-empty",
                                {icons::bell(24, "notification-empty-icon")}
                                div { class: "notification-empty-title", "No notifications" }
                                div { class: "notification-empty-subtitle", "Activity and alerts will appear here." }
                            }
                        } else {
                            for item in &snapshot.activity {
                                {render_notification_row(item, core.clone(), &snapshot.current_workspace)}
                            }
                        }
                    }
                    if !snapshot.current_workspace_log.is_empty() {
                        section { class: "attention-section attention-log",
                            div { class: "attention-section-title", "Workspace log" }
                            div { class: "workspace-log-list",
                                for entry in &snapshot.current_workspace_log {
                                    {render_workspace_log_entry(entry)}
                                }
                            }
                        }
                    }
                }
            }
            if let Some(pointer) = *drag_pointer.read() {
                if snapshot.drag_session.is_some() {
                    {render_drag_preview(&snapshot, pointer)}
                }
            }
        }
    }
}

fn render_workspace_item(
    workspace: &WorkspaceSummary,
    core: SharedCore,
    mut drag_source: Signal<Option<WorkspaceId>>,
    mut drag_target: Signal<Option<WorkspaceId>>,
    dragged_surface: Option<DraggedSurface>,
    mut surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    all_ids: &[WorkspaceId],
) -> Element {
    let tab_class = if workspace.active {
        format!(
            "workspace-tab workspace-tab-active workspace-tab-state-{}",
            workspace.attention.slug()
        )
    } else {
        format!(
            "workspace-tab workspace-tab-state-{}",
            workspace.attention.slug()
        )
    };
    let has_badge = workspace.unread_activity > 0 || workspace.waiting_agent_count > 0;
    let badge_text = if workspace.unread_activity > 0 {
        workspace.unread_activity.to_string()
    } else {
        workspace.waiting_agent_count.to_string()
    };
    let badge_state_class = if workspace.attention == AttentionState::Normal {
        "workspace-unread-badge"
    } else {
        match workspace.attention {
            AttentionState::Error => "workspace-unread-badge workspace-unread-badge-error",
            AttentionState::WaitingInput => "workspace-unread-badge workspace-unread-badge-waiting",
            AttentionState::Completed => "workspace-unread-badge workspace-unread-badge-completed",
            _ => "workspace-unread-badge",
        }
    };
    let workspace_id = workspace.id;
    let focus_workspace = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
        }
    };
    let close_workspace = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CloseWorkspace { workspace_id });
        }
    };

    let branch_row = match (&workspace.git_branch, &workspace.working_directory) {
        (Some(branch), Some(dir)) => Some(format!("{branch} · {dir}")),
        (Some(branch), None) => Some(branch.clone()),
        (None, Some(dir)) => Some(dir.clone()),
        (None, None) => None,
    };
    let ports_row = if workspace.listening_ports.is_empty() {
        None
    } else {
        Some(
            workspace
                .listening_ports
                .iter()
                .map(|port| format!(":{port}"))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let tab_style = workspace
        .custom_color
        .as_ref()
        .map(|color| format!("--workspace-accent: {color};"))
        .unwrap_or_default();
    let runtime_icon_class = format!(
        "workspace-runtime-icon {}",
        runtime_state_class(workspace.runtime.state)
    );

    let is_workspace_drag_target = *drag_target.read() == Some(workspace_id);
    let is_surface_drag_target =
        dragged_surface.is_some_and(|dragged| dragged.preview_workspace_id == workspace_id);
    let outer_class = if is_surface_drag_target {
        "workspace-button workspace-button-surface-drop"
    } else if is_workspace_drag_target {
        "workspace-button workspace-button-drag-over"
    } else {
        "workspace-button"
    };

    let all_ids = all_ids.to_vec();
    let on_dragstart = move |event: Event<DragData>| {
        prime_drag_transfer(&event, WORKSPACE_DRAG_MIME, &workspace_id.to_string());
        drag_source.set(Some(workspace_id));
    };
    let on_dragover = {
        let core = core.clone();
        move |event: Event<DragData>| {
            event.prevent_default();
            event.data().data_transfer().set_drop_effect("move");
            if core.snapshot().surface_drag.is_some() {
                drag_target.set(None);
                surface_drop_target.set(None);
                core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace {
                    workspace_id,
                });
            } else {
                drag_target.set(Some(workspace_id));
            }
        }
    };
    let preview_surface_workspace_enter = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            drag_target.set(None);
            surface_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace { workspace_id });
        }
    };
    let preview_surface_workspace_move = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            drag_target.set(None);
            surface_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace { workspace_id });
        }
    };
    let drop_surface_on_workspace = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let Some(dragged_surface) = core.snapshot().surface_drag else {
                return;
            };
            event.stop_propagation();
            drag_source.set(None);
            drag_target.set(None);
            surface_drop_target.set(None);
            if dragged_surface.workspace_id != workspace_id {
                core.dispatch_shell_action(ShellAction::MoveSurfaceToWorkspace {
                    source_pane_id: dragged_surface.pane_id,
                    surface_id: dragged_surface.surface_id,
                    target_workspace_id: workspace_id,
                });
            }
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };
    let on_dragleave = move |_: Event<DragData>| {
        if *drag_target.read() == Some(workspace_id) {
            drag_target.set(None);
        }
    };
    let on_drop = {
        let core = core.clone();
        let all_ids = all_ids.clone();
        move |event: Event<DragData>| {
            event.prevent_default();
            let dragged_surface = core.snapshot().surface_drag;
            let source = *drag_source.read();
            drag_source.set(None);
            drag_target.set(None);
            surface_drop_target.set(None);
            if let Some(dragged_surface) = dragged_surface {
                if dragged_surface.workspace_id != workspace_id {
                    core.dispatch_shell_action(ShellAction::MoveSurfaceToWorkspace {
                        source_pane_id: dragged_surface.pane_id,
                        surface_id: dragged_surface.surface_id,
                        target_workspace_id: workspace_id,
                    });
                }
                core.dispatch_shell_action(ShellAction::EndDrag);
                return;
            }
            if let Some(source_id) = source {
                if source_id != workspace_id {
                    let mut new_order = all_ids.clone();
                    if let Some(src_pos) = new_order.iter().position(|id| *id == source_id) {
                        new_order.remove(src_pos);
                        let dst_pos = new_order
                            .iter()
                            .position(|id| *id == workspace_id)
                            .unwrap_or(new_order.len());
                        new_order.insert(dst_pos, source_id);
                        core.dispatch_shell_action(ShellAction::ReorderWorkspaces {
                            workspace_ids: new_order,
                        });
                    }
                }
            }
        }
    };

    rsx! {
        button {
            class: "{outer_class}",
            draggable: "true",
            onclick: focus_workspace,
            ondragstart: on_dragstart,
            ondragover: on_dragover,
            ondragleave: on_dragleave,
            ondrop: on_drop,
            onpointerenter: preview_surface_workspace_enter,
            onpointermove: preview_surface_workspace_move,
            onpointerup: drop_surface_on_workspace,
            div { class: "{tab_class}", style: "{tab_style}",
                if workspace.active {
                    div { class: "workspace-tab-rail" }
                }
                div { class: "workspace-tab-content",
                    div { class: "workspace-tab-header",
                        div { class: "workspace-tab-title-row",
                            {render_runtime_icon(&workspace.runtime, 12, &runtime_icon_class)}
                            div { class: "workspace-tab-title", "{workspace.title}" }
                        }
                        div { class: "workspace-tab-trailing",
                            if has_badge {
                                span { class: "{badge_state_class}", "{badge_text}" }
                            }
                            button {
                                class: "workspace-tab-close",
                                onclick: close_workspace,
                                {icons::close(12, "workspace-tab-close-icon")}
                            }
                        }
                    }
                    if let Some(status) = &workspace.status_text {
                        div { class: "workspace-notification workspace-status", "{status}" }
                    } else if let Some(notification) = &workspace.notification_text {
                        div { class: "workspace-notification", "{notification}" }
                    }
                    if let Some(branch) = &branch_row {
                        div { class: "workspace-branch-row",
                            {icons::git_branch(10, "workspace-branch-icon")}
                            span { "{branch}" }
                        }
                    }
                    if let Some(ports) = &ports_row {
                        div { class: "workspace-ports-row",
                            {icons::network(10, "workspace-ports-icon")}
                            span { "{ports}" }
                        }
                    }
                    {render_workspace_progress(&workspace.progress)}
                    {render_workspace_pull_requests(&workspace.pull_requests)}
                }
            }
        }
    }
}

fn render_workspace_progress(progress: &Option<ProgressSnapshot>) -> Element {
    let Some(progress) = progress else {
        return rsx! {};
    };
    let pct = (progress.fraction * 100.0).clamp(0.0, 100.0);
    let fill_style = format!("width: {pct:.1}%;");
    rsx! {
        div { class: "workspace-progress",
            div { class: "workspace-progress-track",
                div { class: "workspace-progress-fill", style: "{fill_style}" }
            }
            if let Some(label) = &progress.label {
                span { class: "workspace-progress-label", "{label}" }
            }
        }
    }
}

fn render_workspace_pull_requests(pull_requests: &[PullRequestSnapshot]) -> Element {
    if pull_requests.is_empty() {
        return rsx! {};
    }
    rsx! {
        for pr in pull_requests {
            div { class: "workspace-pr-row",
                span { class: "workspace-pr-number", "#{pr.number}" }
                span { class: "workspace-pr-title", "{pr.title}" }
            }
        }
    }
}

fn render_workspace_log_entry(entry: &WorkspaceLogEntrySnapshot) -> Element {
    rsx! {
        div { class: "workspace-log-entry",
            div { class: "workspace-log-entry-header",
                if let Some(source) = &entry.source {
                    span { class: "workspace-log-source", "{source}" }
                }
                span { class: "workspace-log-time", "{entry.timestamp}" }
            }
            div { class: "workspace-log-message", "{entry.message}" }
        }
    }
}

#[component]
fn VcsPanel(panel: VcsPanelSnapshot, core: SharedCore) -> Element {
    let commit_message = use_signal(String::new);
    let branch_name = use_signal(String::new);
    let bookmark_name = use_signal(String::new);
    let refresh = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::RefreshVcsPanel)
    };

    rsx! {
        aside { class: "attention-panel vcs-panel",
            div { class: "notification-header",
                div { class: "sidebar-heading", "Version Control" }
                button {
                    class: "notification-jump-button vcs-refresh-button",
                    r#type: "button",
                    onclick: refresh,
                    title: "Refresh repository status",
                    {icons::refresh(12, "browser-toolbar-icon")}
                    span { "Refresh" }
                }
            }

            if let Some(error) = &panel.error {
                div { class: "vcs-panel-error", "{error}" }
            }

            if let Some(snapshot) = &panel.snapshot {
                section { class: "attention-section",
                    div { class: "vcs-panel-summary",
                        div { class: "vcs-panel-repo-row",
                            span { class: "notification-count-pill notification-count-unread", "{format_vcs_mode(snapshot.mode)}" }
                            span { class: "workspace-label", "{snapshot.repo_name}" }
                        }
                        div { class: "workspace-branch-row",
                            {icons::git_branch(10, "workspace-branch-icon")}
                            span { "{snapshot.headline}" }
                        }
                        if let Some(detail) = &snapshot.detail {
                            div { class: "workspace-notification workspace-status", "{detail}" }
                        }
                        div { class: "workspace-notification", "{snapshot.summary_text}" }
                        if let Some(pr) = &snapshot.pull_request {
                            a {
                                class: "workspace-pr-row vcs-pr-link",
                                href: "{pr.url}",
                                target: "_blank",
                                rel: "noreferrer noopener",
                                span { class: "workspace-pr-number",
                                    if let Some(number) = pr.number {
                                        "#{number}"
                                    } else {
                                        "PR"
                                    }
                                }
                                span { class: "workspace-pr-title",
                                    "{pr.title.clone().unwrap_or_else(|| pr.url.clone())}"
                                }
                            }
                        }
                    }
                }

                {render_vcs_action_panel(snapshot, core.clone(), commit_message, branch_name, bookmark_name)}

                section { class: "attention-section vcs-files",
                    div { class: "attention-section-title", "Changed files" }
                    if snapshot.files.is_empty() {
                        div { class: "notification-empty-subtitle", "No changed files." }
                    } else {
                        div { class: "vcs-file-list",
                            for file in &snapshot.files {
                                {render_vcs_file_row(file, snapshot.diff_path.as_deref(), core.clone())}
                            }
                        }
                    }
                }

                section { class: "attention-section vcs-diff",
                    div { class: "attention-section-title", "Diff preview" }
                    if let Some(diff) = &snapshot.diff_text {
                        pre { class: "vcs-diff-preview", "{diff}" }
                    } else {
                        div { class: "notification-empty-subtitle", "Select a file to preview its diff." }
                    }
                }
            } else {
                div { class: "notification-empty",
                    {icons::git_branch(24, "notification-empty-icon")}
                    div { class: "notification-empty-title", "No repository selected" }
                    if let Some(title) = &panel.target_surface_title {
                        div { class: "notification-empty-subtitle",
                            "Focused terminal: {title}"
                        }
                    } else {
                        div { class: "notification-empty-subtitle",
                            "Select a terminal inside a Git or JJ repo to manage version control here."
                        }
                    }
                }
            }
        }
    }
}

fn render_vcs_action_panel(
    snapshot: &VcsSnapshot,
    core: SharedCore,
    mut commit_message: Signal<String>,
    mut branch_name: Signal<String>,
    mut bookmark_name: Signal<String>,
) -> Element {
    let surface_id = snapshot.surface_id;
    let mode = snapshot.mode;
    let fetch = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::RunVcsCommand {
                command: match mode {
                    VcsMode::Git => VcsCommand::GitFetch { surface_id },
                    VcsMode::Jj => VcsCommand::JjFetch { surface_id },
                },
            });
        }
    };
    let push = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::RunVcsCommand {
                command: match mode {
                    VcsMode::Git => VcsCommand::GitPush { surface_id },
                    VcsMode::Jj => VcsCommand::JjPush { surface_id },
                },
            });
        }
    };

    rsx! {
        section { class: "attention-section vcs-actions",
            div { class: "attention-section-title", "Actions" }
            div { class: "vcs-action-row",
                button { class: "pane-action", r#type: "button", onclick: fetch, "Fetch" }
                if snapshot.mode == VcsMode::Git {
                    button {
                        class: "pane-action",
                        r#type: "button",
                        onclick: {
                            let core = core.clone();
                            move |_| {
                                core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                    command: VcsCommand::GitPull { surface_id },
                                });
                            }
                        },
                        "Pull"
                    }
                }
                button { class: "pane-action", r#type: "button", onclick: push, "Push" }
            }

            if snapshot.mode == VcsMode::Git {
                form {
                    class: "vcs-inline-form",
                    onsubmit: {
                        let core = core.clone();
                        move |_| {
                            let message = commit_message.read().trim().to_string();
                            if !message.is_empty() {
                                core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                    command: VcsCommand::GitCommit { surface_id, message },
                                });
                                commit_message.set(String::new());
                            }
                        }
                    },
                    input {
                        class: "browser-address vcs-input",
                        r#type: "text",
                        value: "{commit_message}",
                        placeholder: "Commit message",
                        oninput: move |event| commit_message.set(event.value()),
                    }
                    button { class: "pane-action", r#type: "submit", "Commit" }
                }
                div { class: "vcs-ref-list",
                    for reference in &snapshot.refs {
                        button {
                            class: if reference.active { "pane-action vcs-ref-chip vcs-ref-chip-active" } else { "pane-action vcs-ref-chip" },
                            r#type: "button",
                            onclick: {
                                let core = core.clone();
                                let name = reference.name.clone();
                                move |_| {
                                    core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                        command: VcsCommand::GitSwitchBranch {
                                            surface_id,
                                            name: name.clone(),
                                        },
                                    });
                                }
                            },
                            "{reference.name}"
                        }
                    }
                }
                form {
                    class: "vcs-inline-form",
                    onsubmit: {
                        let core = core.clone();
                        move |_| {
                            let name = branch_name.read().trim().to_string();
                            if !name.is_empty() {
                                core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                    command: VcsCommand::GitCreateBranch { surface_id, name },
                                });
                                branch_name.set(String::new());
                            }
                        }
                    },
                    input {
                        class: "browser-address vcs-input",
                        r#type: "text",
                        value: "{branch_name}",
                        placeholder: "New branch name",
                        oninput: move |event| branch_name.set(event.value()),
                    }
                    button { class: "pane-action", r#type: "submit", "Create branch" }
                }
            } else {
                form {
                    class: "vcs-inline-form",
                    onsubmit: {
                        let core = core.clone();
                        move |_| {
                            let message = commit_message.read().trim().to_string();
                            if !message.is_empty() {
                                core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                    command: VcsCommand::JjDescribe { surface_id, message },
                                });
                                commit_message.set(String::new());
                            }
                        }
                    },
                    input {
                        class: "browser-address vcs-input",
                        r#type: "text",
                        value: "{commit_message}",
                        placeholder: "Change description",
                        oninput: move |event| commit_message.set(event.value()),
                    }
                    button { class: "pane-action", r#type: "submit", "Describe change" }
                }
                button {
                    class: "pane-action",
                    r#type: "button",
                    onclick: {
                        let core = core.clone();
                        move |_| {
                            core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                command: VcsCommand::JjNew {
                                    surface_id,
                                    message: None,
                                },
                            });
                        }
                    },
                    "New change"
                }
                div { class: "vcs-ref-list",
                    for reference in &snapshot.refs {
                        button {
                            class: if reference.active { "pane-action vcs-ref-chip vcs-ref-chip-active" } else { "pane-action vcs-ref-chip" },
                            r#type: "button",
                            onclick: {
                                let core = core.clone();
                                let name = reference.name.clone();
                                move |_| {
                                    core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                        command: VcsCommand::JjSwitchBookmark {
                                            surface_id,
                                            name: name.clone(),
                                        },
                                    });
                                }
                            },
                            "{reference.name}"
                        }
                    }
                }
                form {
                    class: "vcs-inline-form",
                    onsubmit: {
                        let core = core.clone();
                        move |_| {
                            let name = bookmark_name.read().trim().to_string();
                            if !name.is_empty() {
                                core.dispatch_shell_action(ShellAction::RunVcsCommand {
                                    command: VcsCommand::JjCreateBookmark { surface_id, name },
                                });
                                bookmark_name.set(String::new());
                            }
                        }
                    },
                    input {
                        class: "browser-address vcs-input",
                        r#type: "text",
                        value: "{bookmark_name}",
                        placeholder: "New bookmark name",
                        oninput: move |event| bookmark_name.set(event.value()),
                    }
                    button { class: "pane-action", r#type: "submit", "Create bookmark" }
                }
            }
        }
    }
}

fn render_vcs_file_row(
    file: &VcsFileEntry,
    selected_path: Option<&str>,
    core: SharedCore,
) -> Element {
    let selected = selected_path == Some(file.path.as_str());
    let path = file.path.clone();
    let onclick = move |_| {
        core.dispatch_shell_action(ShellAction::ShowVcsDiff {
            path: Some(path.clone()),
        });
    };
    rsx! {
        button {
            class: if selected { "vcs-file-row vcs-file-row-active" } else { "vcs-file-row" },
            r#type: "button",
            onclick: onclick,
            span { class: "workspace-pr-number", "{format_vcs_file_status(file.status, file.staged)}" }
            span { class: "workspace-pr-title", "{file.path}" }
        }
    }
}

fn format_vcs_mode(mode: VcsMode) -> &'static str {
    match mode {
        VcsMode::Git => "Git",
        VcsMode::Jj => "JJ",
    }
}

fn format_vcs_file_status(status: VcsFileStatus, staged: bool) -> &'static str {
    match (status, staged) {
        (VcsFileStatus::Added, true) => "A+",
        (VcsFileStatus::Added, false) => "A",
        (VcsFileStatus::Deleted, true) => "D+",
        (VcsFileStatus::Deleted, false) => "D",
        (VcsFileStatus::Renamed, true) => "R+",
        (VcsFileStatus::Renamed, false) => "R",
        (VcsFileStatus::Copied, true) => "C+",
        (VcsFileStatus::Copied, false) => "C",
        (VcsFileStatus::Untracked, _) => "??",
        (VcsFileStatus::Conflicted, _) => "!!",
        (VcsFileStatus::Changed, true) => "~+",
        (VcsFileStatus::Changed, false) => "~",
        (VcsFileStatus::Modified, true) => "M+",
        (VcsFileStatus::Modified, false) => "M",
    }
}

fn render_surface_workspace_fallback_drop(
    target_workspace_id: WorkspaceId,
    core: SharedCore,
    mut surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    dragged_surface: Option<DraggedSurface>,
) -> Element {
    let active =
        dragged_surface.is_some_and(|dragged| dragged.preview_workspace_id == target_workspace_id);
    let class = if active {
        "workspace-surface-fallback-drop workspace-surface-fallback-drop-active"
    } else {
        "workspace-surface-fallback-drop"
    };
    let preview_workspace_enter = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let Some(dragged) = core.snapshot().surface_drag else {
                return;
            };
            if dragged.workspace_id == target_workspace_id {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace {
                workspace_id: target_workspace_id,
            });
        }
    };
    let preview_workspace_move = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let Some(dragged) = core.snapshot().surface_drag else {
                return;
            };
            if dragged.workspace_id == target_workspace_id {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::PreviewSurfaceDragWorkspace {
                workspace_id: target_workspace_id,
            });
        }
    };
    let move_surface_to_workspace = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let dragged = core.snapshot().surface_drag;
            surface_drop_target.set(None);
            let Some(dragged) = dragged else {
                return;
            };
            event.stop_propagation();
            if dragged.workspace_id != target_workspace_id {
                core.dispatch_shell_action(ShellAction::MoveSurfaceToWorkspace {
                    source_pane_id: dragged.pane_id,
                    surface_id: dragged.surface_id,
                    target_workspace_id,
                });
            }
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };

    rsx! {
        div {
            class: "{class}",
            onpointerenter: preview_workspace_enter,
            onpointermove: preview_workspace_move,
            onpointerup: move_surface_to_workspace,
            div { class: "workspace-surface-fallback-label", "Drop to create a new window" }
        }
    }
}

fn render_layout(
    workspace_id: WorkspaceId,
    node: &LayoutNodeSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    match node {
        LayoutNodeSnapshot::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let direction = match axis {
                SplitAxis::Horizontal => "row",
                SplitAxis::Vertical => "column",
            };
            let first_weight = (*ratio).clamp(0.1, 0.9);
            let second_weight = (1.0 - first_weight).clamp(0.1, 0.9);
            let first_style = format!("flex: {first_weight} 1 0%;");
            let second_style = format!("flex: {second_weight} 1 0%;");

            rsx! {
                div { class: "split-container", style: "flex-direction: {direction};",
                    div { class: "split-child", style: "{first_style}",
                        {render_layout(workspace_id, first, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface, pane_tab_drag_candidate, dragged_pane_tab, pane_tab_drop_target)}
                    }
                    div { class: "split-child", style: "{second_style}",
                        {render_layout(workspace_id, second, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface, pane_tab_drag_candidate, dragged_pane_tab, pane_tab_drop_target)}
                    }
                }
            }
        }
        LayoutNodeSnapshot::Pane(pane) => render_pane(
            workspace_id,
            pane,
            overview_mode,
            browser_chrome,
            core,
            runtime_status,
            surface_drop_target,
            surface_drag_candidate,
            dragged_surface,
            pane_tab_drag_candidate,
            dragged_pane_tab,
            pane_tab_drop_target,
        ),
    }
}

fn render_workspace_strip(
    workspace: &WorkspaceViewSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    window_drag_source: Signal<Option<DraggedWindow>>,
    window_drop_target: Signal<Option<WorkspaceWindowMoveTarget>>,
    window_tab_drag_candidate: Signal<Option<WindowTabDragCandidate>>,
    dragged_window_tab: Option<DraggedWindowTab>,
    window_tab_drop_target: Signal<Option<WindowTabDropTarget>>,
    pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    let viewport_class = if workspace.overview_scale < 1.0 {
        "workspace-viewport workspace-viewport-overview"
    } else {
        "workspace-viewport"
    };
    let scroll_viewport = {
        let core = core.clone();
        let overview_scale = workspace.overview_scale;
        move |event: Event<WheelData>| {
            if overview_scale < 1.0 {
                return;
            }
            let delta = event.delta().strip_units();
            if delta.x.abs() < 1.0 || delta.x.abs() < delta.y.abs() {
                return;
            }
            let dx = delta.x.round() as i32;
            if dx == 0 {
                return;
            }
            event.prevent_default();
            core.dispatch_shell_action(ShellAction::ScrollViewport { dx, dy: 0 });
        }
    };
    let canvas_style = format!(
        "width:{}px;height:{}px;",
        workspace.canvas_width, workspace.canvas_height
    );

    rsx! {
        div { class: "{viewport_class}", onwheel: scroll_viewport,
            div { class: "workspace-strip-canvas", style: "{canvas_style}",
                if dragged_surface.is_some_and(|dragged| dragged.workspace_id != workspace.id)
                {
                    {render_surface_workspace_fallback_drop(
                        workspace.id,
                        core.clone(),
                        surface_drop_target,
                        dragged_surface,
                    )}
                }
                for column in &workspace.columns {
                    for window in &column.windows {
                        {render_workspace_window(
                            window,
                            workspace,
                            overview_mode,
                            browser_chrome,
                            core.clone(),
                            runtime_status,
                            surface_drop_target,
                            surface_drag_candidate,
                            dragged_surface,
                            window_drag_source,
                            window_drop_target,
                            window_tab_drag_candidate,
                            dragged_window_tab,
                            window_tab_drop_target,
                            pane_tab_drag_candidate,
                            dragged_pane_tab,
                            pane_tab_drop_target,
                        )}
                    }
                }
            }
        }
    }
}

fn render_workspace_window(
    window: &WorkspaceWindowSnapshot,
    workspace: &WorkspaceViewSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    mut surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    mut window_drag_source: Signal<Option<DraggedWindow>>,
    window_drop_target: Signal<Option<WorkspaceWindowMoveTarget>>,
    window_tab_drag_candidate: Signal<Option<WindowTabDragCandidate>>,
    dragged_window_tab: Option<DraggedWindowTab>,
    mut window_tab_drop_target: Signal<Option<WindowTabDropTarget>>,
    pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    let local_x = window.frame.x - workspace.viewport_origin_x;
    let local_y = window.frame.y - workspace.viewport_origin_y;
    let style = format!(
        "left:{}px;top:{}px;width:{}px;height:{}px;",
        local_x, local_y, window.frame.width, window.frame.height
    );
    let window_class = if window.active {
        "workspace-window-shell workspace-window-shell-active"
    } else {
        "workspace-window-shell"
    };
    let window_id = window.id;
    let focus_core = core.clone();
    let focus_window = move |_| {
        focus_core.dispatch_shell_action(ShellAction::FocusWorkspaceWindow { window_id });
    };
    let start_window_drag = {
        let core = core.clone();
        move |event: Event<DragData>| {
            prime_drag_transfer(&event, WINDOW_DRAG_MIME, &window_id.to_string());
            surface_drop_target.set(None);
            window_drag_source.set(Some(DraggedWindow { window_id }));
            core.dispatch_shell_action(ShellAction::BeginWindowDrag);
        }
    };
    let clear_window_drag = {
        let core = core.clone();
        let mut window_drag_source = window_drag_source;
        let mut window_drop_target = window_drop_target;
        let mut surface_drop_target = surface_drop_target;
        let mut window_tab_drop_target = window_tab_drop_target;
        move |_: Event<DragData>| {
            window_drag_source.set(None);
            window_drop_target.set(None);
            surface_drop_target.set(None);
            window_tab_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };
    let drag_active = window_drag_source.read().is_some() || dragged_window_tab.is_some();
    let left_target = WorkspaceWindowMoveTarget::ColumnBefore {
        column_id: window.column_id,
    };
    let right_target = WorkspaceWindowMoveTarget::ColumnAfter {
        column_id: window.column_id,
    };
    let top_target = WorkspaceWindowMoveTarget::StackAbove { window_id };
    let bottom_target = WorkspaceWindowMoveTarget::StackBelow { window_id };
    let ordered_window_tab_ids = window.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
    let add_window_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CreateWorkspaceWindowTab { window_id });
        }
    };
    let window_tab_add_class = if *window_tab_drop_target.read()
        == Some(WindowTabDropTarget::AppendToWindow { window_id })
    {
        "workspace-window-tab-add workspace-window-tab-add-active"
    } else {
        "workspace-window-tab-add"
    };

    rsx! {
        section { class: "{window_class}", style: "{style}",
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-left",
                left_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                dragged_window_tab,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-right",
                right_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                dragged_window_tab,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-top",
                top_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                dragged_window_tab,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-bottom",
                bottom_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                dragged_window_tab,
                core.clone(),
            )}
            div { class: "workspace-window-toolbar",
                div { class: "workspace-window-toolbar-tabs",
                    for tab in &window.tabs {
                        {render_workspace_window_tab(
                            window.id,
                            tab,
                            &ordered_window_tab_ids,
                            core.clone(),
                            window_tab_drag_candidate,
                            dragged_window_tab,
                            window_tab_drop_target,
                        )}
                    }
                    button {
                        class: "{window_tab_add_class}",
                        title: "New window tab",
                        onclick: add_window_tab,
                        onpointerenter: move |event: Event<PointerData>| {
                            if dragged_window_tab.is_none() {
                                return;
                            }
                            event.stop_propagation();
                            window_tab_drop_target.set(Some(WindowTabDropTarget::AppendToWindow {
                                window_id,
                            }));
                        },
                        onpointermove: move |event: Event<PointerData>| {
                            if dragged_window_tab.is_none() {
                                return;
                            }
                            event.stop_propagation();
                            window_tab_drop_target.set(Some(WindowTabDropTarget::AppendToWindow {
                                window_id,
                            }));
                        },
                        onpointerleave: move |_: Event<PointerData>| {
                            if *window_tab_drop_target.read()
                                == Some(WindowTabDropTarget::AppendToWindow { window_id })
                            {
                                window_tab_drop_target.set(None);
                            }
                        },
                        onpointerup: move |event: Event<PointerData>| {
                            window_tab_drop_target.set(None);
                            let Some(dragged) = dragged_window_tab else {
                                return;
                            };
                            event.stop_propagation();
                            if dragged.window_id == window_id {
                                core.dispatch_shell_action(ShellAction::MoveWorkspaceWindowTab {
                                    window_id,
                                    tab_id: dragged.tab_id,
                                    target_index: usize::MAX,
                                });
                            } else {
                                core.dispatch_shell_action(ShellAction::TransferWorkspaceWindowTab {
                                    source_window_id: dragged.window_id,
                                    tab_id: dragged.tab_id,
                                    target_window_id: window_id,
                                    target_index: usize::MAX,
                                });
                            }
                            core.dispatch_shell_action(ShellAction::EndDrag);
                        },
                        {icons::plus(12, "workspace-window-tab-add-icon")}
                    }
                }
                div {
                    class: "workspace-window-toolbar-spacer",
                    title: "Drag window",
                    draggable: "true",
                    onclick: focus_window,
                    ondragstart: start_window_drag,
                    ondragend: clear_window_drag,
                }
            }
            div { class: "workspace-window-body",
                {render_layout(
                    workspace.id,
                    &window.layout,
                    overview_mode,
                    browser_chrome,
                    core.clone(),
                    runtime_status,
                    surface_drop_target,
                    surface_drag_candidate,
                    dragged_surface,
                    pane_tab_drag_candidate,
                    dragged_pane_tab,
                    pane_tab_drop_target,
                )}
            }
        }
    }
}

fn render_window_drop_zone(
    base_class: &'static str,
    target: WorkspaceWindowMoveTarget,
    visible: bool,
    mut window_drag_source: Signal<Option<DraggedWindow>>,
    mut window_drop_target: Signal<Option<WorkspaceWindowMoveTarget>>,
    dragged_window_tab: Option<DraggedWindowTab>,
    core: SharedCore,
) -> Element {
    let label = match target {
        WorkspaceWindowMoveTarget::ColumnBefore { .. } => "Column",
        WorkspaceWindowMoveTarget::ColumnAfter { .. } => "Column",
        WorkspaceWindowMoveTarget::StackAbove { .. } => "Stack",
        WorkspaceWindowMoveTarget::StackBelow { .. } => "Stack",
    };
    let class = if *window_drop_target.read() == Some(target) {
        format!("{base_class} workspace-window-drop-zone-active")
    } else if visible {
        format!("{base_class} workspace-window-drop-zone-visible")
    } else {
        base_class.to_string()
    };
    let set_drop_target_drag = move |event: Event<DragData>| {
        if window_drag_source.read().is_none() {
            return;
        }
        mark_move_drop(&event);
        window_drop_target.set(Some(target));
    };
    let set_drop_target_pointer = move |event: Event<PointerData>| {
        if dragged_window_tab.is_none() {
            return;
        }
        event.stop_propagation();
        window_drop_target.set(Some(target));
    };
    let clear_drop_target = move |_: Event<PointerData>| {
        if *window_drop_target.read() == Some(target) {
            window_drop_target.set(None);
        }
    };
    let drag_core = core.clone();
    let drop_window_drag = move |event: Event<DragData>| {
        let dragged_window = *window_drag_source.read();
        if dragged_window.is_none() {
            return;
        }
        event.prevent_default();
        window_drag_source.set(None);
        window_drop_target.set(None);
        if let Some(dragged) = dragged_window {
            drag_core.dispatch_shell_action(ShellAction::MoveWorkspaceWindow {
                window_id: dragged.window_id,
                target,
            });
            drag_core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };
    let pointer_core = core.clone();
    let drop_window_pointer = move |event: Event<PointerData>| {
        let Some(dragged_tab) = dragged_window_tab else {
            return;
        };
        event.stop_propagation();
        window_drop_target.set(None);
        pointer_core.dispatch_shell_action(ShellAction::ExtractWorkspaceWindowTab {
            source_window_id: dragged_tab.window_id,
            tab_id: dragged_tab.tab_id,
            target,
        });
        pointer_core.dispatch_shell_action(ShellAction::EndDrag);
    };

    rsx! {
        div {
            class: "{class}",
            ondragover: set_drop_target_drag,
            onpointerenter: set_drop_target_pointer,
            onpointermove: set_drop_target_pointer,
            ondragleave: move |_: Event<DragData>| {
                if *window_drop_target.read() == Some(target) {
                    window_drop_target.set(None);
                }
            },
            onpointerleave: clear_drop_target,
            ondrop: drop_window_drag,
            onpointerup: drop_window_pointer,
            span { class: "workspace-window-drop-copy", "{label}" }
        }
    }
}

fn render_workspace_window_tab(
    window_id: taskers_core::WorkspaceWindowId,
    tab: &WorkspaceWindowTabSnapshot,
    ordered_tab_ids: &[WorkspaceWindowTabId],
    core: SharedCore,
    mut window_tab_drag_candidate: Signal<Option<WindowTabDragCandidate>>,
    dragged_window_tab: Option<DraggedWindowTab>,
    mut window_tab_drop_target: Signal<Option<WindowTabDropTarget>>,
) -> Element {
    let tab_id = tab.id;
    let is_dragged = dragged_window_tab
        .is_some_and(|dragged| dragged.window_id == window_id && dragged.tab_id == tab_id);
    let is_drop_target = matches!(
        *window_tab_drop_target.read(),
        Some(WindowTabDropTarget::BeforeTab {
            window_id: target_window_id,
            tab_id: target_tab_id,
        }) if target_window_id == window_id && target_tab_id == tab_id
    );
    let attention_class = match tab.attention {
        AttentionState::Completed | AttentionState::WaitingInput | AttentionState::Error => {
            format!(" workspace-window-tab-attention-{}", tab.attention.slug())
        }
        AttentionState::Normal | AttentionState::Busy => String::new(),
    };
    let tab_class = if tab.active {
        format!(
            "workspace-window-tab workspace-window-tab-active{}{}{}",
            attention_class,
            if is_drop_target {
                " workspace-window-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " workspace-window-tab-dragging"
            } else {
                ""
            }
        )
    } else {
        format!(
            "workspace-window-tab{}{}{}",
            attention_class,
            if is_drop_target {
                " workspace-window-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " workspace-window-tab-dragging"
            } else {
                ""
            }
        )
    };
    let runtime_icon_class = format!(
        "workspace-window-tab-kind-icon {}",
        runtime_state_class(tab.runtime.state)
    );
    let tab_title = format!("{} · {}", tab.runtime.label, tab.title);
    let focus_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::FocusWorkspaceWindowTab { window_id, tab_id });
        }
    };
    let close_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CloseWorkspaceWindowTab { window_id, tab_id });
        }
    };
    let begin_tab_drag_candidate = move |event: Event<PointerData>| {
        if let Some(candidate) = window_tab_drag_candidate_from_event(&event, window_id, tab_id) {
            window_tab_drop_target.set(None);
            window_tab_drag_candidate.set(Some(candidate));
        }
    };
    let set_drop_target = move |event: Event<PointerData>| {
        if dragged_window_tab.is_none() {
            return;
        }
        event.stop_propagation();
        window_tab_drop_target.set(Some(WindowTabDropTarget::BeforeTab { window_id, tab_id }));
    };
    let clear_drop_target = move |_: Event<PointerData>| {
        if *window_tab_drop_target.read()
            == Some(WindowTabDropTarget::BeforeTab { window_id, tab_id })
        {
            window_tab_drop_target.set(None);
        }
    };
    let drop_tab = {
        let core = core.clone();
        let ordered_tab_ids = ordered_tab_ids.to_vec();
        move |event: Event<PointerData>| {
            window_tab_drop_target.set(None);
            let Some(dragged) = dragged_window_tab else {
                return;
            };
            event.stop_propagation();
            let target_index =
                compute_window_tab_drop_index(dragged, window_id, &ordered_tab_ids, Some(tab_id));
            if dragged.window_id == window_id {
                core.dispatch_shell_action(ShellAction::MoveWorkspaceWindowTab {
                    window_id,
                    tab_id: dragged.tab_id,
                    target_index,
                });
            } else {
                core.dispatch_shell_action(ShellAction::TransferWorkspaceWindowTab {
                    source_window_id: dragged.window_id,
                    tab_id: dragged.tab_id,
                    target_window_id: window_id,
                    target_index,
                });
            }
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };

    rsx! {
        div {
            class: "{tab_class}",
            title: "{tab_title}",
            onpointerdown: begin_tab_drag_candidate,
            onpointerenter: set_drop_target,
            onpointermove: set_drop_target,
            onpointerleave: clear_drop_target,
            onpointerup: drop_tab,
            button { class: "workspace-window-tab-button", onclick: focus_tab,
                {render_runtime_icon(&tab.runtime, 11, &runtime_icon_class)}
                span { class: "workspace-window-tab-copy",
                    span { class: "workspace-window-tab-title", "{tab.title}" }
                }
            }
            button { class: "workspace-window-tab-close", title: "Close window tab", onclick: close_tab,
                {icons::close(10, "workspace-window-tab-close-icon")}
            }
        }
    }
}

fn render_pane(
    workspace_id: WorkspaceId,
    pane: &PaneSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    mut pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    let pane_id = pane.id;
    let pane_container_id = pane.pane_container_id;
    let ordered_pane_tab_ids = pane
        .pane_tabs
        .iter()
        .map(|pane_tab| pane_tab.id)
        .collect::<Vec<_>>();
    let pane_class = if pane.active {
        "pane-card pane-card-active".to_string()
    } else {
        "pane-card".to_string()
    };
    let focus_pane = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::FocusPane { pane_id });
        }
    };
    let add_browser_pane_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CreatePaneTab {
                pane_container_id,
                kind: PaneKind::Browser,
            })
        }
    };
    let add_terminal_pane_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CreatePaneTab {
                pane_container_id,
                kind: PaneKind::Terminal,
            })
        }
    };
    let add_terminal_pane_tab_plus = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CreatePaneTab {
                pane_container_id,
                kind: PaneKind::Terminal,
            })
        }
    };
    let close_pane_tab = {
        let core = core.clone();
        let active_pane_tab_id = pane.active_pane_tab;
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::ClosePaneTab {
                pane_container_id,
                pane_tab_id: active_pane_tab_id,
            })
        }
    };

    let flash_key = pane.focus_flash_token;
    let flash_class = if flash_key > 0 {
        "pane-flash-ring pane-flash-ring-active"
    } else {
        "pane-flash-ring"
    };
    let pane_tab_add_class = if *pane_tab_drop_target.read()
        == Some(PaneTabDropTarget::AppendToContainer { pane_container_id })
    {
        "surface-tab-add surface-tab-append-target"
    } else {
        "surface-tab-add"
    };

    rsx! {
        div { class: "pane-frame",
            section { class: "{pane_class}", onclick: focus_pane,
                div { class: "pane-toolbar",
                    div { class: "pane-tabs pane-tabs-primary",
                        div { class: "surface-tabs",
                            for pane_tab in &pane.pane_tabs {
                                {render_pane_tab(
                                    pane,
                                    pane_tab,
                                    &ordered_pane_tab_ids,
                                    core.clone(),
                                    pane_tab_drag_candidate,
                                    dragged_pane_tab,
                                    pane_tab_drop_target,
                                )}
                            }
                            button {
                                class: "{pane_tab_add_class}",
                                title: "New terminal pane tab",
                                onclick: add_terminal_pane_tab_plus,
                                onpointerenter: move |event: Event<PointerData>| {
                                    if dragged_pane_tab.is_none() {
                                        return;
                                    }
                                    event.stop_propagation();
                                    pane_tab_drop_target.set(Some(PaneTabDropTarget::AppendToContainer {
                                        pane_container_id,
                                    }));
                                },
                                onpointermove: move |event: Event<PointerData>| {
                                    if dragged_pane_tab.is_none() {
                                        return;
                                    }
                                    event.stop_propagation();
                                    pane_tab_drop_target.set(Some(PaneTabDropTarget::AppendToContainer {
                                        pane_container_id,
                                    }));
                                },
                                onpointerleave: move |_: Event<PointerData>| {
                                    if *pane_tab_drop_target.read()
                                        == Some(PaneTabDropTarget::AppendToContainer { pane_container_id })
                                    {
                                        pane_tab_drop_target.set(None);
                                    }
                                },
                                onpointerup: move |event: Event<PointerData>| {
                                    pane_tab_drop_target.set(None);
                                    let Some(dragged) = dragged_pane_tab else {
                                        return;
                                    };
                                    event.stop_propagation();
                                    if dragged.pane_container_id == pane_container_id {
                                        core.dispatch_shell_action(ShellAction::MovePaneTab {
                                            pane_container_id,
                                            pane_tab_id: dragged.pane_tab_id,
                                            target_index: usize::MAX,
                                        });
                                    } else {
                                        core.dispatch_shell_action(ShellAction::TransferPaneTab {
                                            source_pane_container_id: dragged.pane_container_id,
                                            pane_tab_id: dragged.pane_tab_id,
                                            target_pane_container_id: pane_container_id,
                                            target_index: usize::MAX,
                                        });
                                    }
                                    core.dispatch_shell_action(ShellAction::EndDrag);
                                },
                                {icons::plus(12, "surface-tab-add-icon")}
                            }
                        }
                    }
                    div { class: "pane-action-cluster pane-action-cluster-visible",
                        button { class: "pane-utility", title: "New terminal pane tab", onclick: add_terminal_pane_tab,
                            {icons::terminal(14, "pane-utility-icon")}
                        }
                        button { class: "pane-utility", title: "New browser pane tab", onclick: add_browser_pane_tab,
                            {icons::globe(14, "pane-utility-icon")}
                        }
                        div { class: "pane-action-separator" }
                        button { class: "pane-utility pane-utility-close", title: "Close pane tab", onclick: close_pane_tab,
                            {icons::close(12, "pane-utility-icon")}
                        }
                    }
                }
                {render_pane_tab_layout(
                    workspace_id,
                    &pane.layout,
                    overview_mode,
                    browser_chrome,
                    core.clone(),
                    runtime_status,
                    surface_drop_target,
                    surface_drag_candidate,
                    dragged_surface,
                    pane_tab_drag_candidate,
                    dragged_pane_tab,
                    pane_tab_drop_target,
                )}
                div { key: "{flash_key}", class: "{flash_class}" }
            }
        }
    }
}

fn render_pane_tab(
    pane: &PaneSnapshot,
    pane_tab: &PaneTabSnapshot,
    ordered_tab_ids: &[PaneTabId],
    core: SharedCore,
    mut pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    mut pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    let pane_container_id = pane.pane_container_id;
    let pane_tab_id = pane_tab.id;
    let is_dragged = dragged_pane_tab.is_some_and(|dragged| {
        dragged.pane_container_id == pane_container_id && dragged.pane_tab_id == pane_tab_id
    });
    let is_drop_target = matches!(
        *pane_tab_drop_target.read(),
        Some(PaneTabDropTarget::BeforeTab {
            pane_container_id: target_container_id,
            pane_tab_id: target_tab_id,
        }) if target_container_id == pane_container_id && target_tab_id == pane_tab_id
    );
    let attention_class = match pane_tab.attention {
        AttentionState::WaitingInput => " surface-tab-attention-waiting",
        AttentionState::Error => " surface-tab-attention-error",
        AttentionState::Completed => " surface-tab-attention-completed",
        AttentionState::Normal | AttentionState::Busy => "",
    };
    let tab_class = if pane_tab.active {
        format!(
            "surface-tab surface-tab-active{attention_class}{}{}",
            if is_drop_target {
                " surface-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " surface-tab-dragging"
            } else {
                ""
            }
        )
    } else {
        format!(
            "surface-tab{attention_class}{}{}",
            if is_drop_target {
                " surface-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " surface-tab-dragging"
            } else {
                ""
            }
        )
    };
    let focus_core = core.clone();
    let focus_pane_tab = move |event: Event<MouseData>| {
        event.stop_propagation();
        focus_core.dispatch_shell_action(ShellAction::FocusPaneTab {
            pane_container_id,
            pane_tab_id,
        });
    };
    let close_pane_tab = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::ClosePaneTab {
                pane_container_id,
                pane_tab_id,
            });
        }
    };
    let pane_tab_icon_class = format!(
        "surface-tab-kind-icon {}",
        runtime_state_class(pane_tab.runtime.state)
    );
    let begin_pane_tab_drag_candidate = move |event: Event<PointerData>| {
        if let Some(candidate) =
            pane_tab_drag_candidate_from_event(&event, pane_container_id, pane_tab_id)
        {
            pane_tab_drop_target.set(None);
            pane_tab_drag_candidate.set(Some(candidate));
        }
    };
    let set_drop_target = move |event: Event<PointerData>| {
        if dragged_pane_tab.is_none() {
            return;
        }
        event.stop_propagation();
        pane_tab_drop_target.set(Some(PaneTabDropTarget::BeforeTab {
            pane_container_id,
            pane_tab_id,
        }));
    };
    let clear_drop_target = move |_: Event<PointerData>| {
        if *pane_tab_drop_target.read()
            == Some(PaneTabDropTarget::BeforeTab {
                pane_container_id,
                pane_tab_id,
            })
        {
            pane_tab_drop_target.set(None);
        }
    };
    let drop_pane_tab = {
        let core = core.clone();
        let ordered_tab_ids = ordered_tab_ids.to_vec();
        move |event: Event<PointerData>| {
            pane_tab_drop_target.set(None);
            let Some(dragged) = dragged_pane_tab else {
                return;
            };
            event.stop_propagation();
            let target_index = compute_pane_tab_drop_index(
                dragged,
                pane_container_id,
                &ordered_tab_ids,
                Some(pane_tab_id),
            );
            if dragged.pane_container_id == pane_container_id {
                core.dispatch_shell_action(ShellAction::MovePaneTab {
                    pane_container_id,
                    pane_tab_id: dragged.pane_tab_id,
                    target_index,
                });
            } else {
                core.dispatch_shell_action(ShellAction::TransferPaneTab {
                    source_pane_container_id: dragged.pane_container_id,
                    pane_tab_id: dragged.pane_tab_id,
                    target_pane_container_id: pane_container_id,
                    target_index,
                });
            }
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };

    rsx! {
        div {
            class: "{tab_class}",
            title: "{pane_tab.title}",
            onpointerdown: begin_pane_tab_drag_candidate,
            onpointerenter: set_drop_target,
            onpointermove: set_drop_target,
            onpointerleave: clear_drop_target,
            onpointerup: drop_pane_tab,
            button { class: "surface-tab-focus", onclick: focus_pane_tab,
                {render_runtime_icon(&pane_tab.runtime, 10, &pane_tab_icon_class)}
                span { class: "surface-tab-copy",
                    span { class: "surface-tab-primary", "{pane_tab.title}" }
                }
            }
            button { class: "surface-tab-close", title: "Close pane tab", onclick: close_pane_tab,
                {icons::close(10, "surface-tab-close-icon")}
            }
        }
    }
}

fn render_pane_tab_layout(
    workspace_id: WorkspaceId,
    node: &PaneTabLayoutSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    pane_tab_drag_candidate: Signal<Option<PaneTabDragCandidate>>,
    dragged_pane_tab: Option<DraggedPaneTab>,
    pane_tab_drop_target: Signal<Option<PaneTabDropTarget>>,
) -> Element {
    match node {
        PaneTabLayoutSnapshot::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let direction = match axis {
                SplitAxis::Horizontal => "row",
                SplitAxis::Vertical => "column",
            };
            let first_weight = (*ratio).clamp(0.1, 0.9);
            let second_weight = (1.0 - first_weight).clamp(0.1, 0.9);
            let first_style = format!("flex: {first_weight} 1 0%;");
            let second_style = format!("flex: {second_weight} 1 0%;");

            rsx! {
                div { class: "split-container", style: "flex-direction: {direction};",
                    div { class: "split-child", style: "{first_style}",
                        {render_pane_tab_layout(workspace_id, first, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface, pane_tab_drag_candidate, dragged_pane_tab, pane_tab_drop_target)}
                    }
                    div { class: "split-child", style: "{second_style}",
                        {render_pane_tab_layout(workspace_id, second, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface, pane_tab_drag_candidate, dragged_pane_tab, pane_tab_drop_target)}
                    }
                }
            }
        }
        PaneTabLayoutSnapshot::Pane(pane) => render_live_pane(
            workspace_id,
            pane,
            overview_mode,
            browser_chrome,
            core,
            runtime_status,
            surface_drop_target,
            surface_drag_candidate,
            dragged_surface,
        ),
    }
}

fn render_live_pane(
    workspace_id: WorkspaceId,
    pane: &LivePaneSnapshot,
    overview_mode: bool,
    browser_chrome: Option<&BrowserChromeSnapshot>,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
    surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    mut surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
) -> Element {
    let pane_id = pane.id;
    let active_surface = pane
        .surfaces
        .iter()
        .find(|surface| surface.id == pane.active_surface)
        .unwrap_or_else(|| {
            pane.surfaces
                .first()
                .expect("live pane snapshot should contain surfaces")
        });
    let active_surface_id = active_surface.id;
    let ordered_surface_ids = pane
        .surfaces
        .iter()
        .map(|surface| surface.id)
        .collect::<Vec<_>>();
    let active_browser_chrome = browser_chrome
        .filter(|chrome| chrome.surface_id == active_surface.id)
        .cloned();
    let toolbar_key = active_browser_chrome
        .as_ref()
        .map(|chrome| format!("{}-{}", active_surface.id, chrome.url))
        .or_else(|| {
            active_surface
                .url
                .as_ref()
                .map(|url| format!("{}-{}", active_surface.id, url))
        })
        .unwrap_or_else(|| active_surface.id.to_string());
    let pane_allows_split =
        pane_allows_surface_split(dragged_surface, pane_id, pane.surfaces.len());
    let surface_drag_active = dragged_surface.is_some();
    let live_pane_class = if pane.active {
        "live-pane-shell live-pane-shell-active"
    } else {
        "live-pane-shell"
    };
    let pane_action_cluster_class = if pane.active {
        "pane-action-cluster pane-action-cluster-visible"
    } else {
        "pane-action-cluster"
    };
    let render_live_surfaces_in_overview =
        core.snapshot().settings.render_live_surfaces_in_overview;
    let resize_preview_active = core.snapshot().resize_preview_active;

    let focus_pane = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::FocusPane { pane_id });
        }
    };
    let add_browser_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::AddBrowserSurface {
                pane_id: Some(pane_id),
                profile_mode: taskers_core::BrowserProfileMode::PersistentDefault,
            })
        }
    };
    let add_ephemeral_browser_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::AddBrowserSurface {
                pane_id: Some(pane_id),
                profile_mode: taskers_core::BrowserProfileMode::Ephemeral,
            })
        }
    };
    let add_terminal_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::AddTerminalSurface {
                pane_id: Some(pane_id),
            })
        }
    };
    let add_terminal_surface_plus = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::AddTerminalSurface {
                pane_id: Some(pane_id),
            })
        }
    };
    let split_terminal = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::SplitTerminal {
                pane_id: Some(pane_id),
            })
        }
    };
    let split_down = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::FocusPane { pane_id });
            core.dispatch_shortcut_action(ShortcutAction::SplitDown);
        }
    };
    let close_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CloseSurface {
                pane_id,
                surface_id: active_surface_id,
            })
        }
    };
    let close_label = if pane.surfaces.len() > 1 {
        "Close current surface tab"
    } else {
        "Close current surface"
    };
    let begin_active_surface_drag_candidate = move |event: Event<PointerData>| {
        if let Some(candidate) =
            surface_drag_candidate_from_event(&event, workspace_id, pane_id, active_surface_id)
        {
            surface_drag_candidate.set(Some(candidate));
        }
    };

    rsx! {
        section { class: "{live_pane_class}", onclick: focus_pane,
            div { class: "pane-tabs pane-tabs-inline",
                div {
                    class: "surface-tabs",
                    onpointerdown: begin_active_surface_drag_candidate,
                    for surface in &pane.surfaces {
                        {render_surface_tab(
                            workspace_id,
                            pane.id,
                            pane.active_surface,
                            surface,
                            core.clone(),
                            surface_drop_target,
                            surface_drag_candidate,
                            dragged_surface,
                            &ordered_surface_ids,
                        )}
                    }
                    if surface_drag_active {
                        {render_surface_pane_drop_target(
                            "surface-tab surface-tab-append-target",
                            "+",
                            SurfaceDropTarget::AppendToPane { pane_id },
                            core.clone(),
                            surface_drop_target,
                        )}
                    }
                    button { class: "surface-tab-add", title: "New terminal surface", onclick: add_terminal_surface_plus,
                        {icons::plus(12, "surface-tab-add-icon")}
                    }
                }
                div { class: "{pane_action_cluster_class}",
                    button { class: "pane-utility", title: "New terminal surface", onclick: add_terminal_surface,
                        {icons::terminal(14, "pane-utility-icon")}
                    }
                    button { class: "pane-utility", title: "New browser surface", onclick: add_browser_surface,
                        {icons::globe(14, "pane-utility-icon")}
                    }
                    button { class: "pane-utility", title: "New private browser surface", onclick: add_ephemeral_browser_surface,
                        {icons::shield(14, "pane-utility-icon")}
                    }
                    div { class: "pane-action-separator" }
                    button { class: "pane-utility", title: "Split right", onclick: split_terminal,
                        {icons::split_horizontal(14, "pane-utility-icon")}
                    }
                    button { class: "pane-utility", title: "Split down", onclick: split_down,
                        {icons::split_vertical(14, "pane-utility-icon")}
                    }
                    div { class: "pane-action-separator" }
                    button { class: "pane-utility pane-utility-close", title: "{close_label}", onclick: close_surface,
                        {icons::close(12, "pane-utility-icon")}
                    }
                }
            }
            if matches!(active_surface.kind, SurfaceKind::Browser) {
                BrowserToolbar {
                    key: "{toolbar_key}",
                    surface: active_surface.clone(),
                    chrome: active_browser_chrome,
                    core: core.clone(),
                }
            }
            div { class: "pane-body",
                if show_surface_backdrop(
                    active_surface.kind,
                    overview_mode,
                    render_live_surfaces_in_overview,
                    resize_preview_active,
                ) {
                    {render_surface_backdrop(active_surface, runtime_status)}
                }
                if surface_drag_active {
                    div { class: "pane-drop-overlay",
                        {render_surface_pane_drop_target(
                            "pane-drop-target pane-drop-target-center",
                            "Move",
                            SurfaceDropTarget::AppendToPane { pane_id },
                            core.clone(),
                            surface_drop_target,
                        )}
                        if pane_allows_split {
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-left",
                                "",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Left,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-right",
                                "",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Right,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-top",
                                "",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Up,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-bottom",
                                "",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Down,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                        }
                    }
                }
            }
        }
    }
}

fn render_surface_pane_drop_target(
    base_class: &'static str,
    label: &'static str,
    target: SurfaceDropTarget,
    core: SharedCore,
    mut surface_drop_target: Signal<Option<SurfaceDropTarget>>,
) -> Element {
    let class = if *surface_drop_target.read() == Some(target) {
        format!("{base_class} pane-drop-target-active")
    } else {
        base_class.to_string()
    };
    let set_drop_target_enter = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(Some(target));
        }
    };
    let set_drop_target_move = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(Some(target));
        }
    };
    let clear_drop_target = move |_: Event<PointerData>| {
        if *surface_drop_target.read() == Some(target) {
            surface_drop_target.set(None);
        }
    };
    let drop_surface = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let dragged = core.snapshot().surface_drag;
            surface_drop_target.set(None);
            let Some(dragged) = dragged else {
                return;
            };
            event.stop_propagation();
            apply_surface_drop(&core, dragged, target, &[]);
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };

    rsx! {
        div {
            class: "{class}",
            onpointerenter: set_drop_target_enter,
            onpointermove: set_drop_target_move,
            onpointerleave: clear_drop_target,
            onpointerup: drop_surface,
            span { class: "pane-drop-target-copy", "{label}" }
        }
    }
}

fn render_surface_tab(
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    active_surface_id: SurfaceId,
    surface: &SurfaceSnapshot,
    core: SharedCore,
    mut surface_drop_target: Signal<Option<SurfaceDropTarget>>,
    mut surface_drag_candidate: Signal<Option<SurfaceDragCandidate>>,
    dragged_surface: Option<DraggedSurface>,
    ordered_surface_ids: &[SurfaceId],
) -> Element {
    let surface_id = surface.id;
    let is_dragged = dragged_surface
        .is_some_and(|dragged| dragged.pane_id == pane_id && dragged.surface_id == surface_id);
    let is_drop_target = matches!(
        *surface_drop_target.read(),
        Some(SurfaceDropTarget::BeforeSurface {
            pane_id: target_pane_id,
            surface_id: target_surface_id,
        }) if target_pane_id == pane_id && target_surface_id == surface_id
    );
    let tab_class = if surface.id == active_surface_id {
        format!(
            "surface-tab surface-tab-active{}{}{}",
            attention_ring_class(surface.notification_ring, "surface-tab-attention"),
            if is_drop_target {
                " surface-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " surface-tab-dragging"
            } else {
                ""
            }
        )
    } else {
        format!(
            "surface-tab{}{}{}",
            attention_ring_class(surface.notification_ring, "surface-tab-attention"),
            if is_drop_target {
                " surface-tab-drop-target"
            } else {
                ""
            },
            if is_dragged {
                " surface-tab-dragging"
            } else {
                ""
            }
        )
    };
    let focus_core = core.clone();
    let focus_surface = move |event: Event<MouseData>| {
        event.stop_propagation();
        focus_core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id,
        });
    };
    let begin_surface_drag_candidate = move |event: Event<PointerData>| {
        if let Some(candidate) =
            surface_drag_candidate_from_event(&event, workspace_id, pane_id, surface_id)
        {
            surface_drag_candidate.set(Some(candidate));
        }
    };
    let set_surface_drop_target_enter = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if dragged_surface.is_none() && core.snapshot().surface_drag.is_none() {
                return;
            }
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(Some(SurfaceDropTarget::BeforeSurface {
                pane_id,
                surface_id,
            }));
        }
    };
    let set_surface_drop_target_move = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if dragged_surface.is_none() && core.snapshot().surface_drag.is_none() {
                return;
            }
            if core.snapshot().surface_drag.is_none() {
                return;
            }
            event.stop_propagation();
            surface_drop_target.set(Some(SurfaceDropTarget::BeforeSurface {
                pane_id,
                surface_id,
            }));
        }
    };
    let clear_surface_drop_target = move |_: Event<PointerData>| {
        if *surface_drop_target.read()
            == Some(SurfaceDropTarget::BeforeSurface {
                pane_id,
                surface_id,
            })
        {
            surface_drop_target.set(None);
        }
    };
    let drop_surface = {
        let core = core.clone();
        let ordered_surface_ids = ordered_surface_ids.to_vec();
        move |event: Event<PointerData>| {
            let dragged = core.snapshot().surface_drag;
            surface_drop_target.set(None);
            let Some(dragged) = dragged else {
                return;
            };
            event.stop_propagation();
            apply_surface_drop(
                &core,
                dragged,
                SurfaceDropTarget::BeforeSurface {
                    pane_id,
                    surface_id,
                },
                &ordered_surface_ids,
            );
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };
    let surface_runtime_icon_class = format!(
        "surface-tab-kind-icon {}",
        runtime_state_class(surface.runtime.state)
    );
    let surface_tab_state_class = format!(
        "surface-tab-state {}",
        runtime_state_class(surface.runtime.state)
    );
    let surface_tab_title = surface_summary_title(surface);
    let dismissible_status =
        surface_status_text(surface).is_some() && surface.interrupted_agent_resume.is_none();
    let dismiss_surface_alert = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::DismissSurfaceAlert {
                workspace_id,
                pane_id,
                surface_id,
            });
        }
    };
    let swallow_pointer = move |event: Event<PointerData>| {
        event.stop_propagation();
    };
    let close_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::CloseSurface {
                pane_id,
                surface_id,
            });
        }
    };

    rsx! {
        div {
            key: "{surface_id}",
            class: "{tab_class} surface-tab-draggable",
            title: "{surface_tab_title}",
            button {
                class: "surface-tab-focus",
                onclick: focus_surface,
                onpointerdown: begin_surface_drag_candidate,
                onpointerenter: set_surface_drop_target_enter,
                onpointermove: set_surface_drop_target_move,
                onpointerleave: clear_surface_drop_target,
                onpointerup: drop_surface,
                {render_runtime_icon(&surface.runtime, 10, &surface_runtime_icon_class)}
                span { key: "{surface_id}-copy", class: "surface-tab-copy",
                    span { class: "surface-tab-primary", "{surface_primary_label(surface)}" }
                    if let Some(runtime_label) = surface_runtime_badge_text(surface) {
                        span { key: "{surface_id}-runtime-badge", class: "surface-tab-runtime-badge", "{runtime_label}" }
                    }
                }
            }
            button {
                class: "surface-tab-close",
                title: "Close surface tab",
                onpointerdown: swallow_pointer,
                onpointerup: swallow_pointer,
                onclick: close_surface,
                {icons::close(10, "surface-tab-close-icon")}
            }
            if let Some(status_label) = surface_status_text(surface) {
                if dismissible_status {
                    button {
                        r#type: "button",
                        key: "{surface_id}-status-{status_label}",
                        class: "{surface_tab_state_class} surface-tab-dismiss",
                        title: "Dismiss alert",
                        onpointerdown: swallow_pointer,
                        onpointerup: swallow_pointer,
                        onclick: dismiss_surface_alert,
                        "{status_label}"
                    }
                } else {
                    span {
                        key: "{surface_id}-status-{status_label}",
                        class: "{surface_tab_state_class}",
                        "{status_label}"
                    }
                }
            }
        }
    }
}

#[component]
fn BrowserToolbar(
    surface: SurfaceSnapshot,
    chrome: Option<BrowserChromeSnapshot>,
    core: SharedCore,
) -> Element {
    let initial_url = chrome
        .as_ref()
        .map(|chrome| chrome.url.clone())
        .or_else(|| surface.url.clone())
        .unwrap_or_else(|| taskers_core::DEFAULT_BROWSER_HOME.into());
    let mut address = use_signal(|| initial_url.clone());
    let surface_id = surface.id;
    let can_go_back = chrome
        .as_ref()
        .map(|chrome| chrome.can_go_back)
        .unwrap_or(false);
    let can_go_forward = chrome
        .as_ref()
        .map(|chrome| chrome.can_go_forward)
        .unwrap_or(false);
    let devtools_open = chrome
        .as_ref()
        .map(|chrome| chrome.devtools_open)
        .unwrap_or(false);
    let profile_mode = chrome
        .as_ref()
        .map(|chrome| chrome.profile_mode)
        .unwrap_or(surface.browser_profile_mode);
    let devtools_label = if devtools_open {
        "Hide tools"
    } else {
        "Devtools"
    };
    let clear_data_title = if profile_mode.is_ephemeral() {
        "Clear private browser data"
    } else {
        "Clear stored browser data"
    };

    let navigate = {
        let core = core.clone();
        let address = address.clone();
        move |event: Event<FormData>| {
            event.prevent_default();
            let target = address.read().trim().to_string();
            if target.is_empty() {
                return;
            }
            core.dispatch_shell_action(ShellAction::NavigateBrowser {
                surface_id,
                url: target,
            });
        }
    };
    let go_back = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::BrowserBack { surface_id })
    };
    let go_forward = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::BrowserForward { surface_id })
    };
    let reload = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::BrowserReload { surface_id })
    };
    let toggle_devtools = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ToggleBrowserDevtools { surface_id })
    };
    let clear_data = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ClearBrowserData { surface_id })
    };

    rsx! {
        form { class: "browser-toolbar", onsubmit: navigate,
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                disabled: !can_go_back,
                onclick: go_back,
                title: "Go back",
                {icons::arrow_left(14, "browser-toolbar-icon")}
            }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                disabled: !can_go_forward,
                onclick: go_forward,
                title: "Go forward",
                {icons::arrow_right(14, "browser-toolbar-icon")}
            }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                onclick: reload,
                title: "Reload",
                {icons::refresh(14, "browser-toolbar-icon")}
            }
            input {
                class: "browser-address",
                r#type: "text",
                value: "{address}",
                placeholder: "Enter URL...",
                oninput: move |event| address.set(event.value()),
            }
            if profile_mode.is_ephemeral() {
                div { class: "browser-toolbar-badge",
                    {icons::shield(12, "browser-toolbar-icon")}
                    span { "Private" }
                }
            }
            button {
                r#type: "submit",
                class: "browser-toolbar-button browser-toolbar-button-primary",
                title: "Navigate",
                {icons::arrow_right_circle(14, "browser-toolbar-icon")}
            }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                onclick: clear_data,
                title: "{clear_data_title}",
                {icons::trash(14, "browser-toolbar-icon")}
            }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                onclick: toggle_devtools,
                title: "{devtools_label}",
                if devtools_open {
                    {icons::eye_off(14, "browser-toolbar-icon")}
                } else {
                    {icons::eye(14, "browser-toolbar-icon")}
                }
            }
        }
    }
}

fn render_surface_backdrop(surface: &SurfaceSnapshot, runtime_status: &RuntimeStatus) -> Element {
    match surface.kind {
        SurfaceKind::Browser => {
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        {icons::globe(18, "surface-backdrop-icon")}
                        div { class: "surface-backdrop-title", "{surface.title}" }
                    }
                    if let Some(url) = &surface.url {
                        div { class: "surface-meta",
                            span { class: "surface-chip", "{url}" }
                        }
                    }
                }
            }
        }
        SurfaceKind::Terminal => {
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        {icons::terminal(18, "surface-backdrop-icon")}
                        div { class: "surface-backdrop-title", "{surface.title}" }
                        if let Some(message) = runtime_status.terminal_host.message() {
                            div { class: "surface-backdrop-note", "{message}" }
                        }
                        if let Some(message) = runtime_status.terminal_persistence.message() {
                            div { class: "surface-backdrop-note", "{message}" }
                        }
                    }
                    if let Some(cwd) = &surface.cwd {
                        div { class: "surface-meta",
                            span { class: "surface-chip", "{cwd}" }
                        }
                    }
                }
            }
        }
    }
}

fn render_agent_item(
    agent: &AgentSessionSnapshot,
    core: SharedCore,
    current_workspace: &taskers_core::WorkspaceViewSnapshot,
) -> Element {
    let row_class = format!("activity-item activity-item-state-{}", agent.state.slug());
    let agent_icon_class = format!("agent-kind-icon runtime-state-{}", agent.state.slug());
    let workspace_id = agent.workspace_id;
    let pane_id = agent.pane_id;
    let surface_id = agent.surface_id;
    let current_workspace_id = current_workspace.id;
    let focus_core = core.clone();
    let focus_target = move |_| {
        if workspace_id != current_workspace_id {
            focus_core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
        }
        focus_core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id,
        });
    };
    let dismissible = true;
    let dismiss = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::DismissSurfaceAlert {
                workspace_id,
                pane_id,
                surface_id,
            });
        }
    };

    rsx! {
        div { class: "activity-item-row",
            button { class: "activity-item-button", onclick: focus_target,
                div { class: "{row_class}",
                    div { class: "activity-header",
                        {render_runtime_icon_by_key(agent.agent_kind.as_str(), 12, &agent_icon_class)}
                        div { class: "workspace-label", "{agent.title}" }
                        if !dismissible {
                            div { class: "activity-time", "{agent.state.label()}" }
                        }
                    }
                    div { class: "activity-meta", "{agent.workspace_title} · {agent.agent_kind}" }
                }
            }
            if dismissible {
                button {
                    class: "activity-time activity-item-dismiss activity-item-dismiss-label",
                    title: "Dismiss alert",
                    onclick: dismiss,
                    "{agent.state.label()}"
                }
            }
        }
    }
}

fn render_notification_row(
    item: &ActivityItemSnapshot,
    core: SharedCore,
    _current_workspace: &taskers_core::WorkspaceViewSnapshot,
) -> Element {
    let dot_class = if item.unread {
        "notification-dot notification-dot-unread"
    } else {
        "notification-dot notification-dot-read"
    };
    let activity_id = item.id;
    let dismiss = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::DismissActivity { activity_id });
        }
    };
    let focus_target = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::OpenActivity { activity_id });
        }
    };

    rsx! {
        div { class: "notification-row-button", onclick: focus_target,
            div { class: "notification-row",
                div { class: "{dot_class}" }
                div { class: "notification-row-content",
                    div { class: "notification-row-header",
                        div { class: "notification-title", "{item.title}" }
                        div { class: "notification-timestamp", "{item.timestamp}" }
                    }
                    if let Some(body) = &item.body {
                        div { class: "notification-body", "{body}" }
                    }
                    div { class: "notification-row-footer",
                        if let Some(source) = &item.source_workspace_title {
                            div { class: "notification-source", "{source}" }
                        }
                        button {
                            class: "notification-clear",
                            onclick: dismiss,
                            {icons::close(12, "notification-clear-icon")}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SurfaceDragCandidate, SurfaceKind, attention_ring_class, show_surface_backdrop,
        surface_drag_threshold_reached, surface_primary_label, surface_runtime_badge_text,
        surface_status_text, surface_summary_title,
    };
    use crate::taskers_core::{
        AttentionRingState, AttentionState, BrowserProfileMode, PaneId, RuntimeIdentitySnapshot,
        RuntimeStateSnapshot, SurfaceId, SurfaceSnapshot, WorkspaceId,
    };

    fn sample_surface(
        runtime_key: &str,
        runtime_label: &str,
        title: &str,
        activity_label: Option<&str>,
        status_label: Option<&str>,
        state: RuntimeStateSnapshot,
    ) -> SurfaceSnapshot {
        SurfaceSnapshot {
            id: SurfaceId::new(),
            kind: SurfaceKind::Terminal,
            runtime: RuntimeIdentitySnapshot {
                key: runtime_key.into(),
                label: runtime_label.into(),
                state,
            },
            title: title.into(),
            activity_label: activity_label.map(str::to_owned),
            status_label: status_label.map(str::to_owned),
            url: None,
            browser_profile_mode: BrowserProfileMode::PersistentDefault,
            cwd: None,
            attention: AttentionState::Normal,
            notification_ring: None,
            interrupted_agent_resume: None,
        }
    }

    #[test]
    fn attention_ring_class_uses_expected_state_slug() {
        assert_eq!(
            attention_ring_class(Some(AttentionRingState::Error), "pane-card-attention"),
            " pane-card-attention-error"
        );
        assert_eq!(attention_ring_class(None, "surface-tab-attention"), "");
    }

    #[test]
    fn backdrop_switches_between_live_and_abstract_overview_modes() {
        assert!(!show_surface_backdrop(
            SurfaceKind::Browser,
            false,
            true,
            false
        ));
        assert!(show_surface_backdrop(
            SurfaceKind::Terminal,
            false,
            true,
            false
        ));
        assert!(!show_surface_backdrop(
            SurfaceKind::Browser,
            true,
            true,
            false
        ));
        assert!(!show_surface_backdrop(
            SurfaceKind::Terminal,
            true,
            true,
            false
        ));
        assert!(show_surface_backdrop(
            SurfaceKind::Browser,
            true,
            false,
            false
        ));
        assert!(show_surface_backdrop(
            SurfaceKind::Terminal,
            true,
            false,
            false
        ));
        assert!(show_surface_backdrop(
            SurfaceKind::Browser,
            false,
            true,
            true
        ));
        assert!(show_surface_backdrop(
            SurfaceKind::Terminal,
            false,
            true,
            true
        ));
    }

    #[test]
    fn surface_drag_threshold_requires_real_pointer_motion() {
        let candidate = SurfaceDragCandidate {
            workspace_id: WorkspaceId::new(),
            pane_id: PaneId::new(),
            surface_id: SurfaceId::new(),
            start_x: 100.0,
            start_y: 120.0,
        };

        assert!(!surface_drag_threshold_reached(candidate, 104.0, 123.0));
        assert!(surface_drag_threshold_reached(candidate, 106.0, 120.0));
    }

    #[test]
    fn primary_label_prefers_activity_over_stable_title() {
        let surface = sample_surface(
            "codex",
            "Codex",
            "Codex · taskers/main",
            Some("Summarize recent commits"),
            Some("Awaiting response"),
            RuntimeStateSnapshot::Waiting,
        );

        assert_eq!(surface_primary_label(&surface), "Summarize recent commits");
    }

    #[test]
    fn waiting_status_text_uses_awaiting_response_copy() {
        let surface = sample_surface(
            "codex",
            "Codex",
            "Codex · taskers/main",
            Some("Summarize recent commits"),
            Some("Awaiting response"),
            RuntimeStateSnapshot::Waiting,
        );

        assert_eq!(surface_status_text(&surface), Some("Awaiting response"));
        assert_eq!(
            surface_summary_title(&surface),
            "Codex · Awaiting response · Summarize recent commits · Codex · taskers/main"
        );
    }

    #[test]
    fn idle_surfaces_render_without_status_badge_text() {
        let surface = sample_surface(
            "codex",
            "Codex",
            "taskers/main",
            None,
            None,
            RuntimeStateSnapshot::Idle,
        );

        assert_eq!(surface_status_text(&surface), None);
        assert_eq!(surface_primary_label(&surface), "taskers/main");
        assert_eq!(surface_summary_title(&surface), "Codex · taskers/main");
    }

    #[test]
    fn runtime_badge_shows_for_agent_tabs_when_primary_label_hides_it() {
        let surface = sample_surface(
            "codex",
            "Codex",
            "taskers/main",
            Some("Summarize recent commits"),
            None,
            RuntimeStateSnapshot::Working,
        );

        assert_eq!(surface_runtime_badge_text(&surface), Some("Codex"));
    }

    #[test]
    fn runtime_badge_hides_for_generic_terminal_surfaces() {
        let surface = sample_surface(
            "terminal",
            "Terminal",
            "Terminal",
            None,
            None,
            RuntimeStateSnapshot::Idle,
        );

        assert_eq!(surface_runtime_badge_text(&surface), None);
    }

    #[test]
    fn runtime_badge_hides_when_primary_label_already_mentions_runtime() {
        let surface = sample_surface(
            "codex",
            "Codex",
            "Codex · taskers/main",
            None,
            None,
            RuntimeStateSnapshot::Idle,
        );

        assert_eq!(surface_runtime_badge_text(&surface), None);
    }
}

fn render_settings(settings: &SettingsSnapshot, core: SharedCore) -> Element {
    rsx! {
        div { class: "settings-grid",
            section { class: "settings-card",
                div { class: "sidebar-heading", "Themes" }
                div { class: "settings-copy",
                    "Use the legacy Taskers palette vocabulary as the visual source of truth for the shared shell."
                }
                div { class: "theme-grid",
                    for theme in &settings.theme_options {
                        {render_theme_option(theme, core.clone())}
                    }
                }
            }
            section { class: "settings-card",
                div { class: "sidebar-heading", "Shortcut Presets" }
                div { class: "settings-copy",
                    "Balanced keeps common navigation bound. Power User restores dense directional resizing and split controls."
                }
                div { class: "preset-grid",
                    for preset in &settings.shortcut_presets {
                        {render_shortcut_preset(preset, core.clone())}
                    }
                }
            }
            section { class: "settings-card",
                div { class: "sidebar-heading", "Notifications" }
                div { class: "settings-copy",
                    "Desktop alerts follow the active Taskers lifecycle policy. Manual notifications always alert unless the target is already visible."
                }
                div { class: "settings-toggle-list",
                    {render_notification_preference(
                        "Alert on waiting",
                        "Show a desktop notification when an agent needs input.",
                        settings.notification_preferences.alerts_on_waiting,
                        NotificationPreferenceKey::AlertsOnWaiting,
                        core.clone(),
                    )}
                    {render_notification_preference(
                        "Alert on errors",
                        "Show a desktop notification when an agent exits with an error.",
                        settings.notification_preferences.alerts_on_error,
                        NotificationPreferenceKey::AlertsOnError,
                        core.clone(),
                    )}
                    {render_notification_preference(
                        "Alert on completion",
                        "Show a desktop notification when an agent finishes successfully.",
                        settings.notification_preferences.alerts_on_completed,
                        NotificationPreferenceKey::AlertsOnCompleted,
                        core.clone(),
                    )}
                    {render_notification_preference(
                        "Suppress when visible",
                        "Skip the desktop banner if the target pane is already visible in the focused Taskers window.",
                        settings.notification_preferences.suppress_when_visible,
                        NotificationPreferenceKey::SuppressWhenVisible,
                        core.clone(),
                    )}
                }
            }
            section { class: "settings-card",
                div { class: "sidebar-heading", "Workspace overview" }
                div { class: "settings-copy",
                    "Overview mode can either render the real workspace surfaces or fall back to lighter abstraction cards."
                }
                div { class: "settings-toggle-list",
                    {render_overview_surface_preference(
                        "Render live surfaces",
                        "Show the actual browser and terminal contents in overview mode. Turn this off to use the lower-cost abstraction cards instead.",
                        settings.render_live_surfaces_in_overview,
                        core.clone(),
                    )}
                }
            }
            section { class: "settings-card settings-card-span",
                div { class: "sidebar-heading", "Shortcut Reference" }
                div { class: "shortcut-groups",
                    for category in ["General", "Browser", "Focus", "Top-level windows", "Pane splits", "Advanced resize"] {
                        {render_shortcut_group(category, &settings.shortcuts)}
                    }
                }
            }
        }
    }
}

fn render_theme_option(option: &taskers_core::ThemeOptionSnapshot, core: SharedCore) -> Element {
    let option_id = option.id.clone();
    let select = move |_| {
        core.dispatch_shell_action(ShellAction::SelectTheme {
            theme_id: option_id.clone(),
        })
    };
    let class = if option.active {
        "theme-card theme-card-active"
    } else {
        "theme-card"
    };
    rsx! {
        button { class: "{class}", onclick: select,
            div { class: "workspace-label", "{option.label}" }
            div { class: "workspace-meta", "{option.family}" }
        }
    }
}

fn render_shortcut_preset(
    preset: &taskers_core::ShortcutPresetSnapshot,
    core: SharedCore,
) -> Element {
    let preset_id = preset.id.clone();
    let select = move |_| {
        core.dispatch_shell_action(ShellAction::SelectShortcutPreset {
            preset_id: preset_id.clone(),
        })
    };
    let class = if preset.active {
        "preset-card preset-card-active"
    } else {
        "preset-card"
    };
    rsx! {
        button { class: "{class}", onclick: select,
            div { class: "workspace-label", "{preset.label}" }
            div { class: "settings-copy", "{preset.detail}" }
        }
    }
}

fn render_shortcut_group(category: &'static str, bindings: &[ShortcutBindingSnapshot]) -> Element {
    let entries = bindings
        .iter()
        .filter(|binding| binding.category == category)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return rsx! {};
    }

    rsx! {
        section { class: "shortcut-group",
            div { class: "workspace-label", "{category}" }
            div { class: "shortcut-list",
                for binding in entries {
                    div { class: "shortcut-row",
                        div {
                            div { class: "shortcut-label", "{binding.label}" }
                            div { class: "settings-copy", "{binding.detail}" }
                        }
                        div { class: "shortcut-accelerators",
                            if binding.accelerators.is_empty() {
                                span { class: "shortcut-pill shortcut-pill-muted", "Unbound" }
                            } else {
                                for accelerator in &binding.accelerators {
                                    span { class: "shortcut-pill", "{accelerator}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_notification_preference(
    label: &'static str,
    detail: &'static str,
    enabled: bool,
    key: NotificationPreferenceKey,
    core: SharedCore,
) -> Element {
    let toggle = move |_| {
        core.dispatch_shell_action(ShellAction::SetNotificationPreference {
            key,
            enabled: !enabled,
        })
    };
    let track_class = if enabled {
        "toggle-track toggle-track-active"
    } else {
        "toggle-track"
    };

    rsx! {
        button { class: "settings-toggle-row", onclick: toggle,
            div { class: "settings-toggle-copy",
                div { class: "workspace-label", "{label}" }
                div { class: "settings-copy", "{detail}" }
            }
            div { class: "{track_class}",
                div { class: "toggle-thumb" }
            }
        }
    }
}

fn render_overview_surface_preference(
    label: &'static str,
    detail: &'static str,
    enabled: bool,
    core: SharedCore,
) -> Element {
    let toggle = move |_| {
        core.dispatch_shell_action(ShellAction::SetOverviewLiveSurfaces { enabled: !enabled })
    };
    let track_class = if enabled {
        "toggle-track toggle-track-active"
    } else {
        "toggle-track"
    };

    rsx! {
        button { class: "settings-toggle-row", onclick: toggle,
            div { class: "settings-toggle-copy",
                div { class: "workspace-label", "{label}" }
                div { class: "settings-copy", "{detail}" }
            }
            div { class: "{track_class}",
                div { class: "toggle-thumb" }
            }
        }
    }
}
