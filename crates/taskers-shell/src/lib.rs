mod theme;

use dioxus::html::{
    PointerData,
    input_data::MouseButton,
    point_interaction::{InteractionLocation, PointerInteraction},
};
use dioxus::prelude::*;
use taskers_core::{
    ActivityItemSnapshot, AgentSessionSnapshot, AttentionState, BrowserChromeSnapshot, Direction,
    LayoutNodeSnapshot, NotificationPreferenceKey, PaneId, PaneSnapshot, ProgressSnapshot,
    PullRequestSnapshot, RuntimeStatus, SettingsSnapshot, SharedCore, ShellAction, ShellSection,
    ShellSnapshot, ShortcutAction, ShortcutBindingSnapshot, SplitAxis, SurfaceDragSessionSnapshot,
    SurfaceId, SurfaceKind, SurfaceSnapshot, WorkspaceId, WorkspaceLogEntrySnapshot,
    WorkspaceSummary, WorkspaceViewSnapshot, WorkspaceWindowMoveTarget, WorkspaceWindowSnapshot,
};
use taskers_shell_core as taskers_core;

type DraggedSurface = SurfaceDragSessionSnapshot;

const WORKSPACE_DRAG_MIME: &str = "application/x-taskers-workspace";
const WINDOW_DRAG_MIME: &str = "application/x-taskers-window";
const SURFACE_DRAG_THRESHOLD_PX: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq)]
struct DraggedWindow {
    window_id: taskers_core::WorkspaceWindowId,
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

fn pane_has_surface_drop_target(target: Option<SurfaceDropTarget>, pane_id: PaneId) -> bool {
    match target {
        Some(SurfaceDropTarget::AppendToPane {
            pane_id: target_pane_id,
        })
        | Some(SurfaceDropTarget::BeforeSurface {
            pane_id: target_pane_id,
            ..
        })
        | Some(SurfaceDropTarget::SplitPane {
            pane_id: target_pane_id,
            ..
        }) => target_pane_id == pane_id,
        None => false,
    }
}

fn pane_allows_surface_split(
    dragged: Option<DraggedSurface>,
    pane_id: PaneId,
    surface_count: usize,
) -> bool {
    dragged.is_some_and(|dragged| dragged.pane_id != pane_id || surface_count > 1)
}

fn show_live_surface_backdrop(surface_kind: SurfaceKind, overview_mode: bool) -> bool {
    overview_mode || !matches!(surface_kind, SurfaceKind::Browser)
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

fn surface_drag_threshold_reached(
    candidate: SurfaceDragCandidate,
    current_x: f64,
    current_y: f64,
) -> bool {
    let dx = current_x - candidate.start_x;
    let dy = current_y - candidate.start_y;
    dx.hypot(dy) >= SURFACE_DRAG_THRESHOLD_PX
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
        snapshot.attention_panel_visible,
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
    let show_workspace_nav = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::ShowSection {
                section: ShellSection::Workspace,
            })
        }
    };
    let show_settings_nav = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::ShowSection {
                section: ShellSection::Settings,
            })
        }
    };
    let create_workspace = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::CreateWorkspace)
    };
    let show_active_section_header = {
        let core = core.clone();
        let section = snapshot.section;
        move |_| {
            core.dispatch_shell_action(ShellAction::ShowSection { section });
        }
    };
    let jump_unread = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::FocusLatestUnread)
    };
    let drag_source = use_signal(|| None::<WorkspaceId>);
    let drag_target = use_signal(|| None::<WorkspaceId>);
    let mut surface_drop_target = use_signal(|| None::<SurfaceDropTarget>);
    let mut surface_drag_candidate = use_signal(|| None::<SurfaceDragCandidate>);
    let window_drag_source = use_signal(|| None::<DraggedWindow>);
    let window_drop_target = use_signal(|| None::<WorkspaceWindowMoveTarget>);
    let workspace_ids: Vec<WorkspaceId> = snapshot.workspaces.iter().map(|ws| ws.id).collect();
    let dragged_surface = snapshot.surface_drag;
    let track_surface_drag = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            let candidate = *surface_drag_candidate.read();
            if let Some(candidate) = candidate
                && event.data().held_buttons().contains(MouseButton::Primary)
            {
                let (current_x, current_y) = pointer_client_position(&event);
                if surface_drag_threshold_reached(candidate, current_x, current_y) {
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

            if core.snapshot().surface_drag.is_some()
                && !event.data().held_buttons().contains(MouseButton::Primary)
            {
                surface_drag_candidate.set(None);
                surface_drop_target.set(None);
                core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
                event.stop_propagation();
            }
        }
    };
    let finish_surface_drag_up = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if surface_drag_candidate.read().is_some() {
                surface_drag_candidate.set(None);
            }
            if core.snapshot().surface_drag.is_some() {
                surface_drop_target.set(None);
                core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
                event.stop_propagation();
            }
        }
    };
    let finish_surface_drag_cancel = {
        let core = core.clone();
        move |event: Event<PointerData>| {
            if surface_drag_candidate.read().is_some() {
                surface_drag_candidate.set(None);
            }
            if core.snapshot().surface_drag.is_some() {
                surface_drop_target.set(None);
                core.dispatch_shell_action(ShellAction::CancelSurfaceDrag);
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
            onpointermove: track_surface_drag,
            onpointerup: finish_surface_drag_up,
            onpointercancel: finish_surface_drag_cancel,
            aside { class: "workspace-sidebar",
                div { class: "sidebar-brand",
                    h1 { "Taskers" }
                }
                div { class: "sidebar-nav",
                    button {
                        class: if matches!(snapshot.section, ShellSection::Workspace) { "sidebar-nav-button sidebar-nav-button-active" } else { "sidebar-nav-button" },
                        onclick: show_workspace_nav,
                        "Workspaces"
                    }
                    button {
                        class: if matches!(snapshot.section, ShellSection::Settings) { "sidebar-nav-button sidebar-nav-button-active" } else { "sidebar-nav-button" },
                        onclick: show_settings_nav,
                        "Settings"
                    }
                }
                div { class: "sidebar-section-header",
                    div { class: "sidebar-heading", "Workspaces" }
                    button { class: "workspace-add", onclick: create_workspace, "+" }
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
            }

            main { class: "{main_class}",
                header { class: "workspace-header",
                    div { class: "workspace-header-main",
                        button {
                            class: "workspace-header-title-btn",
                            onclick: show_active_section_header,
                            span { class: "workspace-header-label",
                                if matches!(snapshot.section, ShellSection::Workspace) {
                                    "{snapshot.current_workspace.title}"
                                } else {
                                    "Settings"
                                }
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
                        )}
                    }
                } else {
                    div { class: "settings-canvas",
                        {render_settings(&snapshot.settings, core.clone())}
                    }
                }
            }

            if snapshot.attention_panel_visible {
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
                        if snapshot.activity.is_empty() && snapshot.done_activity.is_empty() {
                            div { class: "notification-empty",
                                div { class: "notification-empty-title", "No notifications" }
                            }
                        } else {
                            for item in &snapshot.activity {
                                {render_notification_row(item, core.clone(), &snapshot.current_workspace)}
                            }
                            for item in snapshot.done_activity.iter().take(8) {
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
                        div { class: "workspace-tab-title", "{workspace.title}" }
                        div { class: "workspace-tab-trailing",
                            if has_badge {
                                span { class: "{badge_state_class}", "{badge_text}" }
                            }
                            button {
                                class: "workspace-tab-close",
                                onclick: close_workspace,
                                "×"
                            }
                        }
                    }
                    if let Some(status) = &workspace.status_text {
                        div { class: "workspace-notification workspace-status", "{status}" }
                    } else if let Some(notification) = &workspace.notification_text {
                        div { class: "workspace-notification", "{notification}" }
                    }
                    if let Some(branch) = &branch_row {
                        div { class: "workspace-branch-row", "{branch}" }
                    }
                    if let Some(ports) = &ports_row {
                        div { class: "workspace-ports-row", "{ports}" }
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
                        {render_layout(workspace_id, first, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface)}
                    }
                    div { class: "split-child", style: "{second_style}",
                        {render_layout(workspace_id, second, overview_mode, browser_chrome, core.clone(), runtime_status, surface_drop_target, surface_drag_candidate, dragged_surface)}
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
        move |_: Event<DragData>| {
            window_drag_source.set(None);
            window_drop_target.set(None);
            surface_drop_target.set(None);
            core.dispatch_shell_action(ShellAction::EndDrag);
        }
    };
    let drag_active = window_drag_source.read().is_some();
    let left_target = WorkspaceWindowMoveTarget::ColumnBefore {
        column_id: window.column_id,
    };
    let right_target = WorkspaceWindowMoveTarget::ColumnAfter {
        column_id: window.column_id,
    };
    let top_target = WorkspaceWindowMoveTarget::StackAbove { window_id };
    let bottom_target = WorkspaceWindowMoveTarget::StackBelow { window_id };

    rsx! {
        section { class: "{window_class}", style: "{style}",
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-left",
                left_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-right",
                right_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-top",
                top_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                core.clone(),
            )}
            {render_window_drop_zone(
                "workspace-window-drop-zone workspace-window-drop-zone-bottom",
                bottom_target,
                drag_active,
                window_drag_source,
                window_drop_target,
                core.clone(),
            )}
            div { class: "workspace-window-toolbar",
                draggable: "true",
                onclick: focus_window,
                ondragstart: start_window_drag,
                ondragend: clear_window_drag,
                div { class: "workspace-window-title",
                    span { class: "workspace-label", "{window.title}" }
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
    core: SharedCore,
) -> Element {
    let class = if *window_drop_target.read() == Some(target) {
        format!("{base_class} workspace-window-drop-zone-active")
    } else if visible {
        format!("{base_class} workspace-window-drop-zone-visible")
    } else {
        base_class.to_string()
    };
    let set_drop_target = move |event: Event<DragData>| {
        if window_drag_source.read().is_none() {
            return;
        }
        mark_move_drop(&event);
        window_drop_target.set(Some(target));
    };
    let clear_drop_target = move |_: Event<DragData>| {
        if *window_drop_target.read() == Some(target) {
            window_drop_target.set(None);
        }
    };
    let drop_window = move |event: Event<DragData>| {
        if window_drag_source.read().is_none() {
            return;
        }
        event.prevent_default();
        let dragged = *window_drag_source.read();
        window_drag_source.set(None);
        window_drop_target.set(None);
        let Some(dragged) = dragged else {
            return;
        };
        core.dispatch_shell_action(ShellAction::MoveWorkspaceWindow {
            window_id: dragged.window_id,
            target,
        });
        core.dispatch_shell_action(ShellAction::EndDrag);
    };

    rsx! {
        div {
            class: "{class}",
            ondragover: set_drop_target,
            ondragleave: clear_drop_target,
            ondrop: drop_window,
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
) -> Element {
    let pane_id = pane.id;
    let pane_is_drop_target = pane_has_surface_drop_target(*surface_drop_target.read(), pane_id);
    let pane_class = if pane.active {
        format!(
            "pane-card pane-card-active{}",
            if pane_is_drop_target {
                " pane-card-drop-target"
            } else {
                ""
            }
        )
    } else {
        format!(
            "pane-card{}",
            if pane_is_drop_target {
                " pane-card-drop-target"
            } else {
                ""
            }
        )
    };
    let active_surface = pane
        .surfaces
        .iter()
        .find(|surface| surface.id == pane.active_surface)
        .unwrap_or_else(|| {
            pane.surfaces
                .first()
                .expect("pane snapshot should contain surfaces")
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
    let pane_kind = match active_surface.kind {
        SurfaceKind::Terminal => "terminal",
        SurfaceKind::Browser => "browser",
    };
    let tab_count_label = if pane.surfaces.len() == 1 {
        "1 tab".to_string()
    } else {
        format!("{} tabs", pane.surfaces.len())
    };
    let pane_allows_split =
        pane_allows_surface_split(dragged_surface, pane_id, pane.surfaces.len());
    let surface_drag_active = dragged_surface.is_some();

    let focus_pane = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::FocusPane { pane_id })
    };
    let add_browser_surface = {
        let core = core.clone();
        move |event: Event<MouseData>| {
            event.stop_propagation();
            core.dispatch_shell_action(ShellAction::AddBrowserSurface {
                pane_id: Some(pane_id),
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

    let flash_key = pane.focus_flash_token;
    let flash_class = if flash_key > 0 {
        "pane-flash-ring pane-flash-ring-active"
    } else {
        "pane-flash-ring"
    };
    let close_label = if pane.surfaces.len() > 1 {
        "Close current tab"
    } else {
        "Close current surface"
    };

    rsx! {
        section { class: "{pane_class}", onclick: focus_pane,
            div { class: "pane-toolbar",
                div { class: "pane-toolbar-meta",
                    span { class: "pane-toolbar-eyebrow", "pane" }
                    span { class: "pane-toolbar-detail", "{pane_kind} · {tab_count_label}" }
                }
                div { class: "pane-action-cluster",
                    button { class: "pane-utility pane-utility-tab", title: "New terminal tab", onclick: add_terminal_surface, "+t" }
                    button { class: "pane-utility pane-utility-tab", title: "New browser tab", onclick: add_browser_surface, "+w" }
                    button { class: "pane-utility pane-utility-split", title: "Split right", onclick: split_terminal, "|r" }
                    button { class: "pane-utility pane-utility-split", title: "Split down", onclick: split_down, "|d" }
                    button { class: "pane-utility pane-utility-close", title: "{close_label}", onclick: close_surface, "x" }
                }
            }
            div { class: "pane-tabs",
                div { class: "surface-tabs",
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
                if show_live_surface_backdrop(active_surface.kind, overview_mode) {
                    {render_surface_backdrop(active_surface, runtime_status)}
                }
                if surface_drag_active {
                    div { class: "pane-drop-overlay",
                        {render_surface_pane_drop_target(
                            "pane-drop-target pane-drop-target-center",
                            "append",
                            SurfaceDropTarget::AppendToPane { pane_id },
                            core.clone(),
                            surface_drop_target,
                        )}
                        if pane_allows_split {
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-left",
                                "split left",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Left,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-right",
                                "split right",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Right,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-top",
                                "split up",
                                SurfaceDropTarget::SplitPane {
                                    pane_id,
                                    direction: Direction::Up,
                                },
                                core.clone(),
                                surface_drop_target,
                            )}
                            {render_surface_pane_drop_target(
                                "pane-drop-target pane-drop-target-edge pane-drop-target-bottom",
                                "split down",
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
            div { key: "{flash_key}", class: "{flash_class}" }
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
            "{label}"
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
    let kind_label = match surface.kind {
        SurfaceKind::Terminal => "term",
        SurfaceKind::Browser => "web",
    };
    let surface_id = surface.id;
    let is_drop_target = matches!(
        *surface_drop_target.read(),
        Some(SurfaceDropTarget::BeforeSurface {
            pane_id: target_pane_id,
            surface_id: target_surface_id,
        }) if target_pane_id == pane_id && target_surface_id == surface_id
    );
    let tab_class = if surface.id == active_surface_id {
        format!(
            "surface-tab surface-tab-active{}",
            if is_drop_target {
                " surface-tab-drop-target"
            } else {
                ""
            }
        )
    } else {
        format!(
            "surface-tab{}",
            if is_drop_target {
                " surface-tab-drop-target"
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
        if event.data().trigger_button() != Some(MouseButton::Primary) {
            return;
        }
        let (start_x, start_y) = pointer_client_position(&event);
        event.stop_propagation();
        surface_drag_candidate.set(Some(SurfaceDragCandidate {
            workspace_id,
            pane_id,
            surface_id,
            start_x,
            start_y,
        }));
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

    rsx! {
        button {
            class: "{tab_class} surface-tab-draggable",
            onclick: focus_surface,
            onpointerdown: begin_surface_drag_candidate,
            onpointerenter: set_surface_drop_target_enter,
            onpointermove: set_surface_drop_target_move,
            onpointerleave: clear_surface_drop_target,
            onpointerup: drop_surface,
            span { class: "surface-tab-label", "{kind_label}" }
            span { class: "surface-tab-title", "{surface.title}" }
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
    let devtools_label = if devtools_open {
        "Hide tools"
    } else {
        "Devtools"
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
    let toggle_devtools =
        move |_| core.dispatch_shell_action(ShellAction::ToggleBrowserDevtools { surface_id });

    rsx! {
        form { class: "browser-toolbar", onsubmit: navigate,
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                disabled: !can_go_back,
                onclick: go_back,
                "←"
            }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                disabled: !can_go_forward,
                onclick: go_forward,
                "→"
            }
            button { r#type: "button", class: "browser-toolbar-button", onclick: reload, "↻" }
            input {
                class: "browser-address",
                r#type: "text",
                value: "{address}",
                oninput: move |event| address.set(event.value()),
            }
            button { r#type: "submit", class: "browser-toolbar-button browser-toolbar-button-primary", "Go" }
            button {
                r#type: "button",
                class: "browser-toolbar-button",
                onclick: toggle_devtools,
                "{devtools_label}"
            }
        }
    }
}

fn render_surface_backdrop(surface: &SurfaceSnapshot, runtime_status: &RuntimeStatus) -> Element {
    match surface.kind {
        SurfaceKind::Browser => {
            let url = surface
                .url
                .clone()
                .unwrap_or_else(|| taskers_core::DEFAULT_BROWSER_HOME.into());
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        div { class: "surface-backdrop-eyebrow", "browser" }
                        div { class: "surface-backdrop-title", "{surface.title}" }
                    }
                    div { class: "surface-meta",
                        span { class: "surface-chip", "URL: {url}" }
                    }
                }
            }
        }
        SurfaceKind::Terminal => {
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        div { class: "surface-backdrop-eyebrow", "terminal" }
                        div { class: "surface-backdrop-title", "{surface.title}" }
                        if let Some(message) = runtime_status.terminal_host.message() {
                            div { class: "surface-backdrop-note", "{message}" }
                        }
                    }
                    div { class: "surface-meta",
                        if let Some(cwd) = &surface.cwd {
                            span { class: "surface-chip", "cwd: {cwd}" }
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
    let workspace_id = agent.workspace_id;
    let pane_id = agent.pane_id;
    let surface_id = agent.surface_id;
    let current_workspace_id = current_workspace.id;
    let focus_target = move |_| {
        if workspace_id != current_workspace_id {
            core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
        }
        core.dispatch_shell_action(ShellAction::FocusSurface {
            pane_id,
            surface_id,
        });
    };

    rsx! {
        button { class: "activity-item-button", onclick: focus_target,
            div { class: "{row_class}",
                div { class: "activity-header",
                    div { class: "workspace-label", "{agent.title}" }
                    div { class: "activity-time", "{agent.state.label()}" }
                }
                div { class: "activity-meta", "{agent.workspace_title} · {agent.agent_kind}" }
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
        button { class: "notification-row-button", onclick: focus_target,
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
                            "×"
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
        SurfaceDragCandidate, SurfaceKind, show_live_surface_backdrop,
        surface_drag_threshold_reached,
    };
    use crate::taskers_core::{PaneId, SurfaceId, WorkspaceId};

    #[test]
    fn live_browser_panes_skip_decorative_backdrop_outside_overview() {
        assert!(!show_live_surface_backdrop(SurfaceKind::Browser, false));
        assert!(show_live_surface_backdrop(SurfaceKind::Browser, true));
        assert!(show_live_surface_backdrop(SurfaceKind::Terminal, false));
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
    let button_class = if enabled {
        "settings-toggle-button settings-toggle-button-active"
    } else {
        "settings-toggle-button"
    };
    let state_label = if enabled { "On" } else { "Off" };

    rsx! {
        button { class: "settings-toggle-row", onclick: toggle,
            div { class: "settings-toggle-copy",
                div { class: "workspace-label", "{label}" }
                div { class: "settings-copy", "{detail}" }
            }
            span { class: "{button_class}", "{state_label}" }
        }
    }
}
