mod theme;

use dioxus::prelude::*;
use taskers_core::{
    ActivityItemSnapshot, AttentionState, LayoutNodeSnapshot, PaneSnapshot, RuntimeCapability,
    RuntimeStatus, SettingsSnapshot, SharedCore, ShellAction, ShellSection, ShellSnapshot,
    ShortcutBindingSnapshot, SplitAxis, SurfaceKind, SurfaceSnapshot, WorkspaceDirection,
    WorkspaceSummary, WorkspaceViewSnapshot, WorkspaceWindowSnapshot,
};

fn app_css(snapshot: &ShellSnapshot) -> String {
    theme::generate_css(&theme::resolve_palette(&snapshot.settings.selected_theme_id))
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
    let stylesheet = app_css(&snapshot);
    let show_workspace_nav = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Workspace,
        })
    };
    let show_workspace_header = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Workspace,
        })
    };
    let show_settings_nav = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Settings,
        })
    };
    let show_settings_header = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ShowSection {
            section: ShellSection::Settings,
        })
    };
    let create_workspace = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::CreateWorkspace)
    };
    let split_terminal = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitTerminal { pane_id: None })
    };
    let split_browser = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None })
    };
    let toggle_overview = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ToggleOverview)
    };
    let scroll_left = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ScrollViewport { dx: -360, dy: 0 })
    };
    let scroll_right = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::ScrollViewport { dx: 360, dy: 0 })
    };
    let create_window_right = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
                direction: WorkspaceDirection::Right,
            })
        }
    };
    let create_window_down = {
        let core = core.clone();
        move |_| {
            core.dispatch_shell_action(ShellAction::CreateWorkspaceWindow {
                direction: WorkspaceDirection::Down,
            })
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
        div { class: "app-shell",
            aside { class: "workspace-sidebar",
                div { class: "sidebar-brand",
                    div { class: "sidebar-heading", "Taskers" }
                    h1 { "Taskers" }
                    div { class: "workspace-preview", "Shared Dioxus shell over native browser and terminal hosts." }
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
                        {render_workspace_item(workspace, core.clone())}
                    }
                }
                div { class: "runtime-card",
                    div { class: "sidebar-heading", "Runtime status" }
                    {render_runtime_capability("Ghostty runtime", &snapshot.runtime_status.ghostty_runtime)}
                    {render_runtime_capability("Shell integration", &snapshot.runtime_status.shell_integration)}
                    {render_runtime_capability("Terminal host", &snapshot.runtime_status.terminal_host)}
                }
            }

            main { class: "{main_class}",
                header { class: "workspace-header",
                    div { class: "workspace-header-main",
                        button {
                            class: "workspace-header-title-btn",
                            onclick: show_workspace_header,
                            span { class: "workspace-header-label", "{snapshot.current_workspace.title}" }
                            span { class: "workspace-header-meta",
                                "{snapshot.current_workspace.pane_count} panes · {snapshot.current_workspace.surface_count} surfaces · revision {snapshot.revision}"
                            }
                        }
                    }
                    div { class: "workspace-header-actions",
                        if matches!(snapshot.section, ShellSection::Workspace) {
                            button {
                                class: if snapshot.overview_mode {
                                    "workspace-header-action workspace-header-action-active"
                                } else {
                                    "workspace-header-action"
                                },
                                onclick: toggle_overview,
                                "Overview"
                            }
                            button { class: "workspace-header-action", onclick: scroll_left, "←" }
                            button { class: "workspace-header-action", onclick: scroll_right, "→" }
                            button { class: "workspace-header-action", onclick: create_window_right, "+ column" }
                            button { class: "workspace-header-action", onclick: create_window_down, "+ stack" }
                            button {
                                class: "workspace-header-action",
                                onclick: split_terminal,
                                "+ split"
                            }
                            button {
                                class: "workspace-header-action workspace-header-action-primary",
                                onclick: split_browser,
                                "+ browser"
                            }
                        } else {
                            button {
                                class: "workspace-header-action workspace-header-action-active",
                                onclick: show_settings_header,
                                "Preferences"
                            }
                        }
                    }
                }

                if matches!(snapshot.section, ShellSection::Workspace) {
                    div { class: if snapshot.overview_mode { "workspace-canvas workspace-canvas-overview" } else { "workspace-canvas" },
                        {render_workspace_strip(&snapshot.current_workspace, core.clone(), &snapshot.runtime_status)}
                    }
                } else {
                    div { class: "settings-canvas",
                        {render_settings(&snapshot.settings, core.clone())}
                    }
                }
            }

            aside { class: "attention-panel",
                div { class: "sidebar-heading", "Attention" }
                div { class: "attention-summary",
                    div { class: "workspace-label", "{snapshot.activity.len()} unread items" }
                    div { class: "workspace-meta", "Focus stays in the shared shell while native hosts report metadata and lifecycle changes back into core." }
                }
                if snapshot.activity.is_empty() {
                    div { class: "empty-state", "No unread items." }
                } else {
                    div { class: "activity-list",
                        for item in &snapshot.activity {
                            {render_activity_item(item, core.clone(), &snapshot.current_workspace)}
                        }
                    }
                }
            }
        }
    }
}

fn render_workspace_item(workspace: &WorkspaceSummary, core: SharedCore) -> Element {
    let attention_class = format!("workspace-item-state-{}", workspace.attention.slug());
    let item_class = if workspace.active {
        format!("workspace-item workspace-item-active {attention_class}")
    } else {
        format!("workspace-item {attention_class}")
    };
    let badge_class = if workspace.attention == AttentionState::Normal {
        "workspace-status-badge".to_string()
    } else {
        format!(
            "workspace-status-badge workspace-status-badge-state-{}",
            workspace.attention.slug()
        )
    };
    let workspace_id = workspace.id;
    let focus_workspace = move |_| {
        core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
    };

    rsx! {
        button { class: "workspace-button", onclick: focus_workspace,
            div { class: "{item_class}",
                div {
                    div { class: "workspace-label", "{workspace.title}" }
                    div { class: "workspace-preview", "{workspace.preview}" }
                    div { class: "workspace-meta",
                        "{workspace.pane_count} panes · {workspace.surface_count} surfaces"
                    }
                }
                div { class: "{badge_class}",
                    if workspace.unread_activity > 0 {
                        "{workspace.unread_activity}"
                    } else {
                        "{workspace.attention.label()}"
                    }
                }
            }
        }
    }
}

fn render_runtime_capability(label: &'static str, capability: &RuntimeCapability) -> Element {
    let class = match capability {
        RuntimeCapability::Ready => "status-pill status-pill-ready",
        RuntimeCapability::Fallback { .. } => "status-pill status-pill-fallback",
        RuntimeCapability::Unavailable { .. } => "status-pill status-pill-unavailable",
    };

    rsx! {
        div { class: "runtime-row",
            div { class: "runtime-status-row",
                span { class: "workspace-preview", "{label}" }
                span { class: "{class}", "{capability.label()}" }
            }
            if let Some(message) = capability.message() {
                div { class: "status-copy", "{message}" }
            }
        }
    }
}

fn render_layout(
    node: &LayoutNodeSnapshot,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
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
                        {render_layout(first, core.clone(), runtime_status)}
                    }
                    div { class: "split-child", style: "{second_style}",
                        {render_layout(second, core.clone(), runtime_status)}
                    }
                }
            }
        }
        LayoutNodeSnapshot::Pane(pane) => render_pane(pane, core, runtime_status),
    }
}

fn render_workspace_strip(
    workspace: &WorkspaceViewSnapshot,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
) -> Element {
    let translate_x = if workspace.overview_scale < 1.0 {
        0
    } else {
        -workspace.viewport_x
    };
    let translate_y = if workspace.overview_scale < 1.0 {
        0
    } else {
        -workspace.viewport_y
    };
    let canvas_style = format!(
        "width:{}px;height:{}px;transform:translate({}px, {}px);",
        workspace.canvas_width, workspace.canvas_height, translate_x, translate_y
    );

    rsx! {
        div { class: "workspace-viewport",
            div { class: "workspace-strip-canvas", style: "{canvas_style}",
                for column in &workspace.columns {
                    for window in &column.windows {
                        {render_workspace_window(window, workspace, core.clone(), runtime_status)}
                    }
                }
            }
        }
    }
}

fn render_workspace_window(
    window: &WorkspaceWindowSnapshot,
    workspace: &WorkspaceViewSnapshot,
    core: SharedCore,
    runtime_status: &RuntimeStatus,
) -> Element {
    let local_x = window.frame.x - workspace.viewport_origin_x;
    let local_y = window.frame.y - workspace.viewport_origin_y;
    let style = format!(
        "left:{}px;top:{}px;width:{}px;height:{}px;",
        local_x, local_y, window.frame.width, window.frame.height
    );
    let window_class = if window.active {
        format!(
            "workspace-window-shell workspace-window-shell-active workspace-window-shell-state-{}",
            window.attention.slug()
        )
    } else {
        format!(
            "workspace-window-shell workspace-window-shell-state-{}",
            window.attention.slug()
        )
    };
    let window_id = window.id;
    let focus_core = core.clone();
    let focus_window = move |_| {
        focus_core.dispatch_shell_action(ShellAction::FocusWorkspaceWindow { window_id });
    };

    rsx! {
        section { class: "{window_class}", style: "{style}",
            div { class: "workspace-window-toolbar",
                button { class: "workspace-window-title", onclick: focus_window,
                    span { class: "workspace-label", "{window.title}" }
                    span { class: "workspace-meta", "{window.pane_count} panes · {window.surface_count} surfaces" }
                }
                div { class: "workspace-window-flags",
                    span { class: format!("status-pill status-pill-inline status-pill-{}", window.attention.slug()), "{window.attention.label()}" }
                }
            }
            div { class: "workspace-window-body",
                {render_layout(&window.layout, core.clone(), runtime_status)}
            }
        }
    }
}

fn render_pane(pane: &PaneSnapshot, core: SharedCore, runtime_status: &RuntimeStatus) -> Element {
    let pane_class = if pane.active {
        format!("pane-card pane-card-active pane-card-state-{}", pane.attention.slug())
    } else {
        format!("pane-card pane-card-state-{}", pane.attention.slug())
    };
    let active_surface = pane
        .surfaces
        .iter()
        .find(|surface| surface.id == pane.active_surface)
        .unwrap_or_else(|| pane.surfaces.first().expect("pane snapshot should contain surfaces"));
    let subtitle = match active_surface.kind {
        SurfaceKind::Terminal => active_surface
            .cwd
            .clone()
            .unwrap_or_else(|| "Embedded terminal".into()),
        SurfaceKind::Browser => active_surface
            .url
            .clone()
            .unwrap_or_else(|| "Native browser surface".into()),
    };
    let status_class = format!("status-dot status-dot-{}", active_surface.attention.slug());
    let pane_id = pane.id;
    let active_surface_id = active_surface.id;

    let focus_pane = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::FocusPane { pane_id })
    };
    let add_browser_surface = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::AddBrowserSurface {
            pane_id: Some(pane_id),
        })
    };
    let add_terminal_surface = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::AddTerminalSurface {
            pane_id: Some(pane_id),
        })
    };
    let split_browser = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitBrowser {
            pane_id: Some(pane_id),
        })
    };
    let split_terminal = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitTerminal {
            pane_id: Some(pane_id),
        })
    };
    let close_surface = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::CloseSurface {
            pane_id,
            surface_id: active_surface_id,
        })
    };

    rsx! {
        section { class: "{pane_class}", onclick: focus_pane,
            div { class: "pane-header",
                div { class: "pane-header-main",
                    span { class: "{status_class}", "●" }
                    div { class: "pane-title-stack",
                        div { class: "pane-title", "{active_surface.title}" }
                        div { class: "pane-meta", "{subtitle}" }
                    }
                }
                div { class: "pane-action-cluster",
                    button { class: "pane-action pane-action-tab", onclick: add_browser_surface, "+ tab" }
                    button { class: "pane-action pane-action-tab", onclick: add_terminal_surface, "+ term" }
                    button { class: "pane-action pane-window-action", onclick: split_browser, "+ web" }
                    button { class: "pane-action pane-split-action", onclick: split_terminal, "+ split" }
                    button { class: "pane-action pane-close-action", onclick: close_surface, "×" }
                }
            }
            div { class: "surface-tabs",
                for surface in &pane.surfaces {
                    {render_surface_tab(pane.id, pane.active_surface, surface, core.clone())}
                }
            }
            div { class: "pane-body",
                {render_surface_backdrop(active_surface, runtime_status)}
            }
        }
    }
}

fn render_surface_tab(
    pane_id: taskers_core::PaneId,
    active_surface_id: taskers_core::SurfaceId,
    surface: &SurfaceSnapshot,
    core: SharedCore,
) -> Element {
    let tab_class = if surface.id == active_surface_id {
        format!(
            "surface-tab surface-tab-active surface-tab-state-{}",
            surface.attention.slug()
        )
    } else {
        format!("surface-tab surface-tab-state-{}", surface.attention.slug())
    };
    let surface_id = surface.id;
    let focus_surface = move |_| {
        core.dispatch_shell_action(ShellAction::FocusSurface { pane_id, surface_id });
    };

    rsx! {
        button { class: "{tab_class}", onclick: focus_surface,
            span { class: "surface-tab-label", "{surface.kind.label()}" }
            span { class: "pane-meta", "{surface.title}" }
        }
    }
}

fn render_surface_backdrop(surface: &SurfaceSnapshot, runtime_status: &RuntimeStatus) -> Element {
    let badge_class = format!("status-pill status-pill-inline status-pill-{}", surface.attention.slug());
    match surface.kind {
        SurfaceKind::Browser => {
            let url = surface
                .url
                .clone()
                .unwrap_or_else(|| "about:blank".into());
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        div { class: "surface-backdrop-eyebrow", "Browser surface" }
                        div { class: "surface-backdrop-title", "{surface.title}" }
                        div { class: "surface-backdrop-note",
                            "The platform host mounts a native browser view here while the shared shell keeps tabs, workspace chrome, settings, and activity state consistent."
                        }
                    }
                    div { class: "surface-meta",
                        span { class: "{badge_class}", "{surface.attention.label()}" }
                        span { class: "surface-chip", "URL: {url}" }
                    }
                }
            }
        }
        SurfaceKind::Terminal => {
            let host_message = runtime_status
                .terminal_host
                .message()
                .unwrap_or("Embedded terminal hosting is ready.");
            rsx! {
                div { class: "surface-backdrop",
                    div { class: "surface-backdrop-copy",
                        div { class: "surface-backdrop-eyebrow", "Terminal surface" }
                        div { class: "surface-backdrop-title", "{surface.title}" }
                        div { class: "surface-backdrop-note", "{host_message}" }
                    }
                    div { class: "surface-meta",
                        span { class: "{badge_class}", "{surface.attention.label()}" }
                        if let Some(cwd) = &surface.cwd {
                            span { class: "surface-chip", "cwd: {cwd}" }
                        }
                    }
                }
            }
        }
    }
}

fn render_activity_item(
    item: &ActivityItemSnapshot,
    core: SharedCore,
    current_workspace: &taskers_core::WorkspaceViewSnapshot,
) -> Element {
    let row_class = format!("activity-item activity-item-state-{}", item.attention.slug());
    let activity_id = item.id;
    let dismiss = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::DismissActivity { activity_id })
    };
    let focus_target = {
        let core = core.clone();
        let workspace_id = item.workspace_id;
        let pane_id = item.pane_id;
        let surface_id = item.surface_id;
        let current_workspace_id = current_workspace.id;
        move |_| {
            if workspace_id != current_workspace_id {
                core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
            } else if let (Some(pane_id), Some(surface_id)) = (pane_id, surface_id) {
                core.dispatch_shell_action(ShellAction::FocusSurface { pane_id, surface_id });
            } else if let Some(pane_id) = pane_id {
                core.dispatch_shell_action(ShellAction::FocusPane { pane_id });
            } else {
                core.dispatch_shell_action(ShellAction::FocusWorkspace { workspace_id });
            }
        }
    };

    rsx! {
        div { class: "activity-item-shell",
            button { class: "activity-item-button", onclick: focus_target,
                div { class: "{row_class}",
                    div { class: "activity-header",
                        div { class: "workspace-label", "{item.title}" }
                        div { class: "activity-time", "{item.attention.label()}" }
                    }
                    div { class: "activity-meta", "{item.meta}" }
                    div { class: "activity-preview", "{item.preview}" }
                }
            }
            button { class: "activity-action", onclick: dismiss, "Done" }
        }
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

fn render_theme_option(
    option: &taskers_core::ThemeOptionSnapshot,
    core: SharedCore,
) -> Element {
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

fn render_shortcut_group(
    category: &'static str,
    bindings: &[ShortcutBindingSnapshot],
) -> Element {
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
