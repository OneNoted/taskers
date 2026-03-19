use dioxus::prelude::*;
use std::sync::Arc;
use taskers_core::{
    LayoutNodeSnapshot, PaneId, RuntimeCapability, RuntimeStatus, SharedCore, SplitAxis,
    SurfaceKind,
};

const APP_CSS: &str = r#"
html, body, #main {
  margin: 0;
  width: 100%;
  height: 100%;
  background:
    radial-gradient(circle at top right, rgba(94, 173, 255, 0.18), transparent 32%),
    linear-gradient(180deg, #0b1020 0%, #0d1326 52%, #10182f 100%);
  color: #f4f7fb;
  font-family: "IBM Plex Sans", "Segoe UI", sans-serif;
}
* { box-sizing: border-box; }
button { font: inherit; }
.app-shell {
  width: 100vw;
  height: 100vh;
  display: flex;
  overflow: hidden;
}
.sidebar {
  width: 276px;
  padding: 18px 16px;
  background: rgba(8, 13, 28, 0.78);
  border-right: 1px solid rgba(163, 191, 255, 0.12);
  backdrop-filter: blur(28px);
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.brand {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.eyebrow {
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: #8fb7ff;
}
.brand h1 {
  margin: 0;
  font-size: 28px;
  line-height: 1;
}
.sidebar-card {
  padding: 14px;
  border-radius: 16px;
  background: rgba(18, 28, 53, 0.78);
  border: 1px solid rgba(163, 191, 255, 0.1);
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.workspace-pill {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 12px;
  border-radius: 14px;
  background: linear-gradient(135deg, rgba(58, 104, 206, 0.35), rgba(35, 48, 92, 0.45));
  border: 1px solid rgba(163, 191, 255, 0.18);
}
.workspace-pill strong { font-size: 15px; }
.workspace-pill span, .sidebar-card p, .toolbar-subtitle, .status-note, .placeholder-note {
  color: #b4c7ec;
}
.status-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}
.status-label {
  font-size: 13px;
  color: #dce8ff;
}
.status-badge {
  border-radius: 999px;
  padding: 4px 9px;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.status-badge.ready {
  background: rgba(91, 224, 160, 0.14);
  color: #8cf1be;
}
.status-badge.fallback {
  background: rgba(255, 197, 87, 0.14);
  color: #ffd37f;
}
.status-badge.unavailable {
  background: rgba(255, 128, 128, 0.14);
  color: #ffb4b4;
}
.status-note {
  font-size: 12px;
  line-height: 1.45;
}
.main-column {
  min-width: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}
.toolbar {
  height: 64px;
  padding: 12px 18px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  border-bottom: 1px solid rgba(163, 191, 255, 0.08);
  background: rgba(8, 12, 24, 0.48);
  backdrop-filter: blur(20px);
}
.toolbar-title {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.toolbar-title strong { font-size: 16px; }
.toolbar-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}
.toolbar button {
  border: 0;
  border-radius: 999px;
  padding: 10px 14px;
  background: #d7e7ff;
  color: #102040;
  font-weight: 700;
  cursor: pointer;
}
.toolbar button.secondary {
  background: rgba(255, 255, 255, 0.08);
  color: #eef4ff;
}
.workspace-canvas {
  flex: 1;
  min-height: 0;
  padding: 16px;
}
.split-container {
  width: 100%;
  height: 100%;
  display: flex;
  gap: 12px;
  min-width: 0;
  min-height: 0;
}
.split-child {
  min-width: 0;
  min-height: 0;
}
.pane {
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  border-radius: 20px;
  background: rgba(10, 16, 32, 0.92);
  border: 1px solid rgba(163, 191, 255, 0.08);
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.03);
  overflow: hidden;
}
.pane-active {
  border-color: rgba(130, 187, 255, 0.55);
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.05),
    0 0 0 1px rgba(61, 151, 255, 0.25);
}
.pane-header {
  height: 38px;
  min-height: 38px;
  padding: 0 14px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  border-bottom: 1px solid rgba(163, 191, 255, 0.08);
  background: linear-gradient(180deg, rgba(22, 33, 63, 0.92), rgba(14, 22, 42, 0.92));
}
.pane-title {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}
.pane-title strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.kind-badge {
  border-radius: 999px;
  padding: 4px 8px;
  background: rgba(143, 183, 255, 0.12);
  color: #a9c7ff;
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}
.pane-body {
  flex: 1;
  min-height: 0;
  position: relative;
  overflow: hidden;
}
.surface-placeholder {
  width: 100%;
  height: 100%;
  padding: 20px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.surface-placeholder.browser {
  background:
    linear-gradient(135deg, rgba(30, 61, 120, 0.18), rgba(13, 20, 39, 0.05)),
    radial-gradient(circle at bottom right, rgba(87, 197, 255, 0.12), transparent 30%);
}
.surface-placeholder.terminal {
  background:
    linear-gradient(180deg, rgba(9, 12, 21, 0.9), rgba(12, 18, 34, 0.96));
}
.surface-backdrop {
  width: 100%;
  height: 100%;
  border-radius: 0 0 20px 20px;
  border: 1px dashed rgba(144, 184, 255, 0.12);
}
.surface-backdrop.browser {
  background:
    linear-gradient(180deg, rgba(11, 16, 29, 0.15), rgba(11, 16, 29, 0.02));
}
.surface-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.surface-chip {
  border-radius: 999px;
  padding: 6px 10px;
  font-size: 12px;
  background: rgba(255, 255, 255, 0.06);
  color: #d9e8ff;
}
@media (max-width: 900px) {
  .sidebar { display: none; }
  .workspace-canvas { padding: 12px; }
  .toolbar { padding: 12px; }
}
"#;

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

    let focus = {
        let core = core.clone();
        Arc::new(move |pane_id: PaneId| core.focus_pane(pane_id))
    };

    let split_terminal = {
        let core = core.clone();
        move |_| core.split_with_terminal()
    };

    let split_browser = {
        let core = core.clone();
        move |_| core.split_with_browser()
    };

    rsx! {
        style { "{APP_CSS}" }
        div { class: "app-shell",
            aside { class: "sidebar",
                div { class: "brand",
                    span { class: "eyebrow", "Greenfield rewrite" }
                    h1 { "Taskers" }
                }
                div { class: "workspace-pill",
                    strong { "{snapshot.workspace_title}" }
                    span { "{snapshot.workspace_count} workspace · revision {snapshot.revision}" }
                }
                div { class: "sidebar-card",
                    div { class: "eyebrow", "Portal runtime" }
                    p { "Browser panes are mounted through the Linux GTK portal layer. Terminal startup is bootstrapped too, but native Ghostty mounting is still blocked by the GTK3/GTK4 split." }
                }
                div { class: "sidebar-card",
                    div { class: "eyebrow", "Runtime status" }
                    {render_runtime_capability("Ghostty runtime", &snapshot.runtime_status.ghostty_runtime)}
                    {render_runtime_capability("Shell integration", &snapshot.runtime_status.shell_integration)}
                    {render_runtime_capability("Terminal host", &snapshot.runtime_status.terminal_host)}
                }
            }

            main { class: "main-column",
                header { class: "toolbar",
                    div { class: "toolbar-title",
                        strong { "Unified shell bootstrap" }
                        div { class: "toolbar-subtitle",
                            "Dioxus chrome + Linux GTK portal runtime"
                        }
                    }
                    div { class: "toolbar-actions",
                        button { class: "secondary", onclick: split_terminal, "Split Terminal" }
                        button { onclick: split_browser, "Split Browser" }
                    }
                }

                div { class: "workspace-canvas",
                    {render_layout(&snapshot.layout, focus.clone(), &snapshot.runtime_status)}
                }
            }
        }
    }
}

fn render_runtime_capability(label: &'static str, capability: &RuntimeCapability) -> Element {
    let class = match capability {
        RuntimeCapability::Ready => "status-badge ready",
        RuntimeCapability::Fallback { .. } => "status-badge fallback",
        RuntimeCapability::Unavailable { .. } => "status-badge unavailable",
    };

    rsx! {
        div {
            div { class: "status-row",
                span { class: "status-label", "{label}" }
                span { class: "{class}", "{capability.label()}" }
            }
            if let Some(message) = capability.message() {
                div { class: "status-note", "{message}" }
            }
        }
    }
}

fn render_layout(
    node: &LayoutNodeSnapshot,
    focus: Arc<dyn Fn(PaneId) + 'static>,
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
                        {render_layout(first, focus.clone(), runtime_status)}
                    }
                    div { class: "split-child", style: "{second_style}",
                        {render_layout(second, focus.clone(), runtime_status)}
                    }
                }
            }
        }
        LayoutNodeSnapshot::Pane(pane) => {
            let pane_class = if pane.active {
                "pane pane-active"
            } else {
                "pane"
            };
            let pane_id = pane.id;
            let focus_this = focus.clone();
            let kind_label = pane.surface.kind.label();
            let placeholder = match pane.surface.kind {
                SurfaceKind::Browser => {
                    let url = pane
                        .surface
                        .url
                        .clone()
                        .unwrap_or_else(|| "about:blank".into());
                    rsx! {
                        div { class: "surface-placeholder browser",
                            div { class: "eyebrow", "Native browser surface" }
                            p { class: "placeholder-note",
                                "This pane is mounted through the Linux GTK portal overlay."
                            }
                            div { class: "surface-meta",
                                span { class: "surface-chip", "URL: {url}" }
                            }
                            div { class: "surface-backdrop browser" }
                        }
                    }
                }
                SurfaceKind::Terminal => {
                    let host_message = runtime_status
                        .terminal_host
                        .message()
                        .unwrap_or("Terminal hosting is ready.");
                    rsx! {
                        div { class: "surface-placeholder terminal",
                            div { class: "eyebrow", "Terminal runtime" }
                            p { class: "placeholder-note",
                                "{host_message}"
                            }
                            if let Some(cwd) = &pane.surface.cwd {
                                div { class: "surface-meta",
                                    span { class: "surface-chip", "cwd: {cwd}" }
                                }
                            }
                            div { class: "surface-backdrop" }
                        }
                    }
                }
            };

            rsx! {
                div { class: "{pane_class}", onclick: move |_| (focus_this)(pane_id),
                    div { class: "pane-header",
                        div { class: "pane-title",
                            span { class: "kind-badge", "{kind_label}" }
                            strong { "{pane.surface.title}" }
                        }
                        span { class: "eyebrow", "{pane.id}" }
                    }
                    div { class: "pane-body",
                        {placeholder}
                    }
                }
            }
        }
    }
}
