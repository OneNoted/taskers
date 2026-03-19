mod theme;

use dioxus::prelude::*;
use taskers_core::{
    LayoutNodeSnapshot, RuntimeCapability, RuntimeStatus, SharedCore, ShellAction, SplitAxis,
    SurfaceKind,
};

fn app_css() -> String {
    theme::generate_css(&theme::default_dark())
}

pub fn app() -> Element {
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
    let stylesheet = app_css();
    let focus_active = {
        let core = core.clone();
        let active_pane = snapshot.active_pane;
        move |_| core.dispatch_shell_action(ShellAction::FocusPane {
            pane_id: active_pane,
        })
    };
    let split_terminal = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitTerminal { pane_id: None })
    };
    let split_browser = {
        let core = core.clone();
        move |_| core.dispatch_shell_action(ShellAction::SplitBrowser { pane_id: None })
    };

    rsx! {
        style { "{stylesheet}" }
        div { class: "app-shell",
            aside { class: "workspace-sidebar",
                div { class: "sidebar-brand",
                    div { class: "sidebar-heading", "Taskers shell" }
                    h1 { "Taskers" }
                }
                div { class: "workspace-list",
                    button { class: "workspace-button",
                        div { class: "workspace-item workspace-item-active",
                            div {
                                div { class: "workspace-label", "{snapshot.workspace_title}" }
                                div { class: "workspace-preview", "Unified Dioxus shell over native platform hosts." }
                                div { class: "workspace-meta", "{snapshot.workspace_count} workspace · revision {snapshot.revision}" }
                            }
                            div { class: "workspace-status-badge", "{snapshot.portal.panes.len()}" }
                        }
                    }
                }
                div { class: "runtime-card",
                    div { class: "sidebar-heading", "Shell notes" }
                    div { class: "status-copy",
                        "The shell chrome is shared Dioxus. Browser and terminal pane bodies are mounted by the platform host."
                    }
                }
                div { class: "runtime-card",
                    div { class: "sidebar-heading", "Runtime status" }
                    {render_runtime_capability("Ghostty runtime", &snapshot.runtime_status.ghostty_runtime)}
                    {render_runtime_capability("Shell integration", &snapshot.runtime_status.shell_integration)}
                    {render_runtime_capability("Terminal host", &snapshot.runtime_status.terminal_host)}
                }
            }

            main { class: "workspace-main",
                header { class: "workspace-header",
                    button {
                        class: "workspace-header-title-btn",
                        onclick: focus_active,
                        span { class: "workspace-header-label", "{snapshot.workspace_title}" }
                        span { class: "workspace-header-meta", "Shared shell · native pane bodies" }
                    }
                    div { class: "workspace-header-actions",
                        button {
                            class: "workspace-header-action",
                            onclick: split_terminal,
                            "+ terminal"
                        }
                        button {
                            class: "workspace-header-action workspace-header-action-primary",
                            onclick: split_browser,
                            "+ browser"
                        }
                    }
                }

                div { class: "workspace-canvas",
                    {render_layout(&snapshot.layout, core.clone(), &snapshot.runtime_status)}
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
        div {
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
        LayoutNodeSnapshot::Pane(pane) => {
            let pane_class = if pane.active {
                "pane-card pane-card-active"
            } else {
                "pane-card"
            };
            let pane_id = pane.id;
            let surface_id = pane.surface.id;
            let kind_label = pane.surface.kind.label();
            let surface_copy = match pane.surface.kind {
                SurfaceKind::Browser => {
                    let url = pane
                        .surface
                        .url
                        .clone()
                        .unwrap_or_else(|| "about:blank".into());
                    rsx! {
                        div { class: "surface-backdrop",
                            div { class: "surface-backdrop-copy",
                                div { class: "surface-backdrop-eyebrow", "Browser surface" }
                                div { class: "surface-backdrop-title", "{pane.surface.title}" }
                                div { class: "surface-backdrop-note",
                                    "The platform host mounts a native browser view into this body region while the shared shell keeps the chrome and actions consistent."
                                }
                            }
                            div { class: "surface-meta",
                                span { class: "surface-chip", "URL: {url}" }
                            }
                        }
                    }
                }
                SurfaceKind::Terminal => {
                    let host_message = runtime_status
                        .terminal_host
                        .message()
                        .unwrap_or("Terminal hosting is ready.");
                    rsx! {
                        div { class: "surface-backdrop",
                            div { class: "surface-backdrop-copy",
                                div { class: "surface-backdrop-eyebrow", "Terminal surface" }
                                div { class: "surface-backdrop-title", "{pane.surface.title}" }
                                div { class: "surface-backdrop-note", "{host_message}" }
                            }
                            if let Some(cwd) = &pane.surface.cwd {
                                div { class: "surface-meta",
                                    span { class: "surface-chip", "cwd: {cwd}" }
                                }
                            }
                        }
                    }
                }
            };
            let status_class = match pane.surface.kind {
                SurfaceKind::Browser => "status-dot status-dot-busy",
                SurfaceKind::Terminal => {
                    if matches!(runtime_status.terminal_host, RuntimeCapability::Ready) {
                        "status-dot status-dot-completed"
                    } else {
                        "status-dot status-dot-waiting"
                    }
                }
            };
            let subtitle = match &pane.surface.cwd {
                Some(cwd) => format!("{kind_label} · {cwd}"),
                None => format!("{kind_label} · {}", pane.id),
            };
            let focus_pane = {
                let core = core.clone();
                move |_| core.dispatch_shell_action(ShellAction::FocusPane { pane_id })
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
            let close_pane = {
                let core = core.clone();
                move |_| core.dispatch_shell_action(ShellAction::ClosePane {
                    pane_id,
                    surface_id,
                })
            };

            rsx! {
                section {
                    class: "{pane_class}",
                    onclick: focus_pane,
                    div { class: "pane-header",
                        div { class: "pane-header-main",
                            span { class: "{status_class}", "●" }
                            div { class: "pane-title-stack",
                                div { class: "pane-title", "{pane.surface.title}" }
                                div { class: "pane-meta", "{subtitle}" }
                            }
                        }
                        div { class: "pane-action-cluster",
                            button {
                                class: "pane-action pane-window-action",
                                onclick: split_browser,
                                "+web"
                            }
                            button {
                                class: "pane-action pane-split-action",
                                onclick: split_terminal,
                                "+term"
                            }
                            button {
                                class: "pane-action pane-close-action",
                                onclick: close_pane,
                                "×"
                            }
                        }
                    }
                    div { class: "surface-tabs",
                        div { class: "surface-tab surface-tab-active",
                            span { class: "surface-tab-label", "{kind_label}" }
                            span { class: "pane-meta", "{pane.surface.id}" }
                        }
                    }
                    div { class: "pane-body",
                        {surface_copy}
                    }
                }
            }
        }
    }
}
