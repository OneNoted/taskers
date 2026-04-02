use std::fmt::Write as _;
use taskers_core::LayoutMetrics;
use taskers_shell_core as taskers_core;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemePalette {
    pub base: Color,
    pub surface: Color,
    pub elevated: Color,
    pub overlay: Color,
    pub text: Color,
    pub text_bright: Color,
    pub text_muted: Color,
    pub text_subtle: Color,
    pub text_dim: Color,
    pub text_faint: Color,
    pub border: Color,
    pub accent: Color,
    pub busy: Color,
    pub completed: Color,
    pub waiting: Color,
    pub error: Color,
    pub busy_text: Color,
    pub completed_text: Color,
    pub waiting_text: Color,
    pub error_text: Color,
    pub action_window: Color,
    pub action_split: Color,
    pub action_teal: Color,
}

pub fn resolve_palette(theme_id: &str) -> ThemePalette {
    match theme_id {
        "catppuccin-mocha" => catppuccin_mocha(),
        "tokyo-night" => tokyo_night(),
        "gruvbox-dark" => gruvbox_dark(),
        _ => default_dark(),
    }
}

pub fn default_dark() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x0f, 0x11, 0x17),
        surface: Color::new(0x0d, 0x0f, 0x15),
        elevated: Color::new(0x12, 0x14, 0x1c),
        overlay: Color::new(0x1a, 0x1d, 0x28),
        text: Color::new(0xe2, 0xe4, 0xea),
        text_bright: Color::new(0xf0, 0xf2, 0xf8),
        text_muted: Color::new(0x8b, 0x8f, 0xa3),
        text_subtle: Color::new(0xb0, 0xb4, 0xc4),
        text_dim: Color::new(0x5c, 0x61, 0x78),
        text_faint: Color::new(0x3d, 0x42, 0x59),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x7c, 0x8a, 0xff),
        busy: Color::new(0x7c, 0x8a, 0xff),
        completed: Color::new(0x34, 0xd3, 0x99),
        waiting: Color::new(0x60, 0xa5, 0xfa),
        error: Color::new(0xf8, 0x71, 0x71),
        busy_text: Color::new(0xc7, 0xd2, 0xfe),
        completed_text: Color::new(0xa7, 0xf3, 0xd0),
        waiting_text: Color::new(0xdb, 0xea, 0xfe),
        error_text: Color::new(0xfe, 0xca, 0xca),
        action_window: Color::new(0x7d, 0xd3, 0xfc),
        action_split: Color::new(0x5e, 0xea, 0xd4),
        action_teal: Color::new(0x2d, 0xd4, 0xbf),
    }
}

fn catppuccin_mocha() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x1e, 0x1e, 0x2e),
        surface: Color::new(0x18, 0x18, 0x25),
        elevated: Color::new(0x31, 0x32, 0x44),
        overlay: Color::new(0x45, 0x47, 0x5a),
        text: Color::new(0xcd, 0xd6, 0xf4),
        text_bright: Color::new(0xe4, 0xe8, 0xfb),
        text_muted: Color::new(0xa6, 0xad, 0xc8),
        text_subtle: Color::new(0x93, 0x99, 0xb2),
        text_dim: Color::new(0x7f, 0x84, 0x9c),
        text_faint: Color::new(0x6c, 0x70, 0x86),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xb4, 0xbe, 0xfe),
        busy: Color::new(0x89, 0xb4, 0xfa),
        completed: Color::new(0xa6, 0xe3, 0xa1),
        waiting: Color::new(0x94, 0xe2, 0xd5),
        error: Color::new(0xf3, 0x8b, 0xa8),
        busy_text: Color::new(0xbc, 0xd3, 0xfc),
        completed_text: Color::new(0xc3, 0xed, 0xbe),
        waiting_text: Color::new(0xb8, 0xed, 0xe6),
        error_text: Color::new(0xf7, 0xb8, 0xc8),
        action_window: Color::new(0xcb, 0xa6, 0xf7),
        action_split: Color::new(0x89, 0xdc, 0xeb),
        action_teal: Color::new(0x94, 0xe2, 0xd5),
    }
}

fn tokyo_night() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x1a, 0x1b, 0x26),
        surface: Color::new(0x16, 0x16, 0x1e),
        elevated: Color::new(0x29, 0x2e, 0x42),
        overlay: Color::new(0x41, 0x48, 0x68),
        text: Color::new(0xc0, 0xca, 0xf5),
        text_bright: Color::new(0xdc, 0xe0, 0xf8),
        text_muted: Color::new(0xa9, 0xb1, 0xd6),
        text_subtle: Color::new(0x73, 0x7a, 0xa2),
        text_dim: Color::new(0x56, 0x5f, 0x89),
        text_faint: Color::new(0x3b, 0x42, 0x61),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x7d, 0xcf, 0xff),
        busy: Color::new(0x7a, 0xa2, 0xf7),
        completed: Color::new(0x9e, 0xce, 0x6a),
        waiting: Color::new(0x7d, 0xcf, 0xff),
        error: Color::new(0xf7, 0x76, 0x8e),
        busy_text: Color::new(0xb0, 0xc8, 0xfa),
        completed_text: Color::new(0xc4, 0xe4, 0xa6),
        waiting_text: Color::new(0xb0, 0xe3, 0xff),
        error_text: Color::new(0xfa, 0xb0, 0xbc),
        action_window: Color::new(0xbb, 0x9a, 0xf7),
        action_split: Color::new(0x2a, 0xc3, 0xde),
        action_teal: Color::new(0x1a, 0xbc, 0x9c),
    }
}

fn gruvbox_dark() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x28, 0x28, 0x28),
        surface: Color::new(0x1d, 0x20, 0x21),
        elevated: Color::new(0x3c, 0x38, 0x36),
        overlay: Color::new(0x50, 0x49, 0x45),
        text: Color::new(0xeb, 0xdb, 0xb2),
        text_bright: Color::new(0xfb, 0xf1, 0xc7),
        text_muted: Color::new(0xd5, 0xc4, 0xa1),
        text_subtle: Color::new(0xbd, 0xae, 0x93),
        text_dim: Color::new(0xa8, 0x99, 0x84),
        text_faint: Color::new(0x92, 0x83, 0x74),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x83, 0xa5, 0x98),
        busy: Color::new(0x83, 0xa5, 0x98),
        completed: Color::new(0xb8, 0xbb, 0x26),
        waiting: Color::new(0x8e, 0xc0, 0x7c),
        error: Color::new(0xfb, 0x49, 0x34),
        busy_text: Color::new(0xb4, 0xcf, 0xc5),
        completed_text: Color::new(0xd5, 0xd7, 0x8a),
        waiting_text: Color::new(0xbc, 0xdb, 0xac),
        error_text: Color::new(0xfc, 0xa0, 0x9a),
        action_window: Color::new(0xd3, 0x86, 0x9b),
        action_split: Color::new(0x8e, 0xc0, 0x7c),
        action_teal: Color::new(0x68, 0x9d, 0x6a),
    }
}

fn rgba(color: Color, alpha: f32) -> String {
    format!("rgba({},{},{},{alpha:.2})", color.r, color.g, color.b)
}

pub fn generate_css(
    p: &ThemePalette,
    metrics: LayoutMetrics,
    attention_panel_visible: bool,
) -> String {
    let sidebar_width = metrics.sidebar_width;
    let activity_width = metrics.activity_width;
    let workspace_toolbar_height = metrics.toolbar_height;
    let window_border_width = metrics.window_border_width;
    let window_toolbar_height = metrics.window_toolbar_height;
    let window_body_padding = metrics.window_body_padding;
    let pane_border_width = metrics.pane_border_width;
    let pane_header_height = metrics.pane_header_height;
    let surface_tab_height = metrics.surface_tab_height;
    let browser_toolbar_height = metrics.browser_toolbar_height;
    let split_gap = metrics.split_gap;
    let app_shell_columns = if attention_panel_visible {
        format!("{sidebar_width}px minmax(0, 1fr) {activity_width}px")
    } else {
        format!("{sidebar_width}px minmax(0, 1fr)")
    };
    let mut css = String::with_capacity(22_000);
    let _ = write!(
        css,
        r#"
html, body, #main {{
  margin: 0;
  width: 100%;
  height: 100%;
  background: {base};
  color: {text};
  font-family: "IBM Plex Sans", "SF Pro Text", system-ui, sans-serif;
}}

* {{
  box-sizing: border-box;
}}

button {{
  font: inherit;
  appearance: none;
  -webkit-appearance: none;
  border-radius: 0;
  background: transparent;
  border: none;
  padding: 0;
  color: inherit;
  cursor: pointer;
}}

input {{
  font: inherit;
  appearance: none;
  -webkit-appearance: none;
  border-radius: 0;
  background: transparent;
  border: none;
  padding: 0;
  color: inherit;
}}

button:focus-visible,
input:focus-visible {{
  outline: 2px solid {accent};
  outline-offset: 1px;
}}

::-webkit-scrollbar {{ width: 6px; }}
::-webkit-scrollbar-track {{ background: transparent; }}
::-webkit-scrollbar-thumb {{ background: {border_10}; border-radius: 3px; }}
::-webkit-scrollbar-thumb:hover {{ background: {border_12}; }}

.app-shell {{
  width: 100vw;
  height: 100vh;
  background: {base};
  display: grid;
  grid-template-columns: {app_shell_columns};
  overflow: hidden;
}}

.workspace-sidebar,
.attention-panel {{
  background: {surface_85};
  display: flex;
  flex-direction: column;
  min-height: 0;
  position: relative;
  overflow: hidden;
  isolation: isolate;
  contain: paint;
}}

.workspace-sidebar {{
  border-right: 1px solid {border_05};
  padding: 4px;
  gap: 4px;
  backdrop-filter: blur(12px) saturate(1.4);
}}

.attention-panel {{
  border-left: 1px solid {border_05};
  padding: 8px 10px;
  gap: 6px;
  backdrop-filter: blur(12px) saturate(1.4);
}}

.vcs-panel {{
  gap: 10px;
}}

.sidebar-top {{
  display: flex;
  align-items: center;
  justify-content: flex-end;
  padding: 2px 0;
}}

.sidebar-heading {{
  font-weight: 600;
  font-size: 11px;
  color: {text_dim};
  letter-spacing: 0.10em;
  text-transform: uppercase;
}}

.activity-list {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}

.workspace-list {{
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
  flex: 1;
  min-height: 0;
}}

.sidebar-footer {{
  margin-top: auto;
  padding-top: 6px;
  border-top: 1px solid {border_06};
}}

.sidebar-settings-btn {{
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: {text_dim};
}}

.sidebar-settings-btn:hover {{
  background: {border_06};
  color: {text_bright};
}}

.sidebar-settings-btn-active {{
  background: {accent_14};
  color: {text_bright};
}}

.workspace-button,
.theme-card,
.preset-card {{
  width: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
}}

.workspace-add {{
  background: transparent;
  color: {text_dim};
  border: 1px solid {border_10};
  min-width: 24px;
  min-height: 24px;
  padding: 0;
  font-size: 16px;
  border-radius: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}}

.workspace-add:hover {{
  background: {waiting_10};
  color: {waiting_text};
  border-color: {waiting_18};
}}

.workspace-button[draggable] {{
  cursor: grab;
}}

.workspace-button-drag-over .workspace-tab {{
  border-top: 2px solid var(--workspace-accent, {accent});
}}

.workspace-button-surface-drop .workspace-tab {{
  background: {accent_12};
  border-color: {accent_24};
}}

.workspace-tab {{
  position: relative;
  padding: 8px 10px 8px 14px;
  border: 1px solid transparent;
  border-radius: 0;
  display: flex;
  align-items: stretch;
  gap: 0;
  transition: background 0.14s ease-in-out, border-color 0.14s ease-in-out;
}}

.workspace-button:hover .workspace-tab {{
  background: {border_05};
  border-color: {border_08};
}}

.workspace-tab-active {{
  background: {accent_08};
}}

.workspace-button:hover .workspace-tab-active {{
  background: {accent_12};
}}

.workspace-tab-state-busy {{
  border-color: {busy_12};
}}

.workspace-tab-state-completed {{
  border-color: {completed_12};
}}

.workspace-tab-state-waiting {{
  border-color: {waiting_14};
}}

.workspace-tab-state-error {{
  border-color: {error_12};
}}

.workspace-tab-rail {{
  position: absolute;
  left: 0;
  top: 0;
  bottom: 0;
  width: 2px;
  background: var(--workspace-accent, {accent});
}}

.workspace-tab-content {{
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}}

.workspace-tab-header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}}

.workspace-tab-title-row {{
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 6px;
}}

.workspace-tab-title {{
  font-weight: 600;
  font-size: 12.5px;
  color: {text_bright};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}}

.workspace-tab-trailing {{
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 4px;
}}

.workspace-tab-close {{
  width: 16px;
  height: 16px;
  border: 0;
  background: transparent;
  color: {text_dim};
  font-size: 13px;
  line-height: 1;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  visibility: hidden;
  border-radius: 0;
  transition: background 0.14s ease-in-out, color 0.14s ease-in-out;
}}

.workspace-button:hover .workspace-tab-close {{
  visibility: visible;
}}

.workspace-tab-close:hover {{
  background: {error_16};
  color: {error};
}}

.workspace-unread-badge {{
  flex: 0 0 auto;
  width: 16px;
  height: 16px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 9px;
  font-weight: 700;
  background: var(--workspace-accent, {accent});
  color: {base};
  border-radius: 9999px;
}}

.workspace-unread-badge-error {{
  background: {error};
}}

.workspace-unread-badge-waiting {{
  background: {waiting};
}}

.workspace-unread-badge-completed {{
  background: {completed};
}}

.workspace-notification {{
  color: {text_subtle};
  font-size: 10px;
  line-height: 1.35;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
  overflow: hidden;
}}

.workspace-status {{
  color: {text_bright};
}}

.workspace-branch-row {{
  color: {text_muted};
  font-size: 10px;
  font-family: "IBM Plex Mono", ui-monospace, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  display: flex;
  align-items: center;
  gap: 4px;
}}

.workspace-ports-row {{
  color: {text_dim};
  font-size: 10px;
  font-family: "IBM Plex Mono", ui-monospace, monospace;
  display: flex;
  align-items: center;
  gap: 4px;
}}

.sidebar-settings-icon {{
  flex: 0 0 auto;
  opacity: 0.7;
}}

.sidebar-settings-btn:hover .sidebar-settings-icon,
.sidebar-settings-btn-active .sidebar-settings-icon {{
  opacity: 1.0;
}}

.workspace-add-icon,
.workspace-tab-close-icon,
.workspace-branch-icon,
.workspace-ports-icon,
.workspace-runtime-icon,
.workspace-window-runtime-icon {{
  flex: 0 0 auto;
  display: block;
}}

.workspace-branch-icon,
.workspace-ports-icon {{
  opacity: 0.5;
}}

.runtime-state-idle {{
  color: {text_dim};
}}

.runtime-state-working {{
  color: {busy};
}}

.runtime-state-waiting {{
  color: {waiting};
}}

.runtime-state-completed {{
  color: {completed};
}}

.runtime-state-failed {{
  color: {error};
}}

.workspace-progress {{
  display: flex;
  align-items: center;
  gap: 6px;
}}

.workspace-progress-track {{
  flex: 1;
  height: 3px;
  background: {border_08};
  border-radius: 2px;
}}

.workspace-progress-fill {{
  height: 100%;
  background: var(--workspace-accent, {accent});
  transition: width 0.3s ease;
  border-radius: 2px;
}}

.workspace-progress-label {{
  font-size: 10px;
  color: {text_dim};
  white-space: nowrap;
}}

.workspace-pr-row {{
  display: flex;
  align-items: center;
  gap: 5px;
  font-size: 10px;
}}

.workspace-pr-icon {{
  font-size: 10px;
}}

.workspace-pr-status-Open {{
  color: {completed};
}}

.workspace-pr-status-Draft {{
  color: {waiting};
}}

.workspace-pr-status-Merged {{
  color: {accent};
}}

.workspace-pr-status-Closed {{
  color: {error};
}}

.workspace-pr-number {{
  color: {text_muted};
  font-family: "IBM Plex Mono", ui-monospace, monospace;
}}

.workspace-pr-title {{
  color: {text_dim};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}}

.workspace-label {{
  font-weight: 600;
  font-size: 12.5px;
  color: {text_bright};
}}

.workspace-preview {{
  color: {text_subtle};
  font-size: 12px;
  line-height: 1.35;
}}

.workspace-meta,
.activity-meta,
.activity-time {{
  color: {text_dim};
  font-size: 11px;
}}

.runtime-card,
.settings-card {{
  background: transparent;
  border: 1px solid {border_06};
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  border-radius: 0;
  box-shadow: 0 2px 8px rgba(0,0,0,0.24), inset 0 1px 0 rgba(255,255,255,0.03);
}}

.runtime-row,
.attention-summary {{
  display: flex;
  flex-direction: column;
  gap: 4px;
}}

.settings-toggle-list {{
  display: flex;
  flex-direction: column;
  gap: 8px;
}}

.settings-toggle-row {{
  border: 1px solid {border_06};
  background: {surface};
  padding: 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  text-align: left;
  border-radius: 0;
}}

.settings-toggle-row:hover {{
  border-color: {accent_24};
  background: {accent_08};
}}

.settings-toggle-copy {{
  display: flex;
  flex-direction: column;
  gap: 4px;
}}

.toggle-track {{
  width: 36px;
  height: 20px;
  border-radius: 10px;
  background: {border_10};
  position: relative;
  transition: background 0.14s ease-in-out;
  flex: 0 0 auto;
}}

.toggle-track-active {{
  background: {accent};
}}

.toggle-thumb {{
  position: absolute;
  top: 2px;
  left: 2px;
  width: 16px;
  height: 16px;
  border-radius: 9999px;
  background: {text_bright};
  transition: left 0.14s ease-in-out;
}}

.toggle-track-active .toggle-thumb {{
  left: 18px;
}}

.runtime-status-row {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.status-pill {{
  display: inline-flex;
  align-items: center;
  padding: 4px 8px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  border-radius: 0;
}}

.status-pill-inline {{
  letter-spacing: normal;
  text-transform: none;
  font-size: 11px;
}}

.status-pill-ready,
.status-pill-completed {{
  background: {completed_16};
  color: {completed_text};
}}

.status-pill-fallback,
.status-pill-waiting {{
  background: {waiting_18};
  color: {waiting_text};
}}

.status-pill-unavailable,
.status-pill-error {{
  background: {error_16};
  color: {error_text};
}}

.status-pill-busy {{
  background: {busy_16};
  color: {busy_text};
}}

.status-copy,
.settings-copy,
.activity-preview {{
  color: {text_subtle};
  font-size: 12px;
  line-height: 1.4;
}}

.workspace-main {{
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: {base};
  position: relative;
  overflow: hidden;
  isolation: isolate;
  contain: paint;
}}

.workspace-main-overview .workspace-canvas {{
  background:
    radial-gradient(circle at top left, {accent_08} 0%, transparent 32%),
    linear-gradient(180deg, {border_05} 0%, {base} 100%);
}}

.workspace-header {{
  height: {workspace_toolbar_height}px;
  min-height: {workspace_toolbar_height}px;
  border-bottom: 1px solid {border_08};
  padding: 0 12px;
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 10px;
  background: {base};
}}

.workspace-header-main,
.pane-header-main,
.pane-action-cluster,
.surface-meta,
.activity-header,
.activity-item-shell,
.shortcut-row,
.workspace-header-actions {{
  display: flex;
  align-items: center;
  gap: 8px;
}}

.workspace-header-main,
.shortcut-row {{
  justify-content: flex-start;
}}

.workspace-header-label {{
  display: block;
  font-weight: 600;
  font-size: 12px;
  letter-spacing: 0.02em;
  color: {text_bright};
}}

.workspace-header-actions {{
  margin-left: auto;
}}

.workspace-header-action {{
  min-height: 24px;
  padding: 0 9px;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 11px;
}}

.workspace-header-action-active {{
  background: {accent_14};
  color: {text_bright};
  border-color: {accent_24};
}}

.workspace-header-action-icon {{
  flex: 0 0 auto;
  display: block;
}}

.pane-action,
.activity-action,
.shortcut-pill {{
  border: 1px solid {border_10};
  background: transparent;
  border-radius: 0;
}}

.pane-action,
.activity-action {{
  min-height: 28px;
  padding: 0 10px;
  color: {text_subtle};
  border-radius: 0;
}}

.pane-action:hover {{
  background: {border_06};
  color: {text_bright};
  box-shadow: 0 1px 2px rgba(0,0,0,0.20);
}}

.activity-action-passive {{
  display: inline-flex;
  align-items: center;
  color: {text_dim};
  background: {border_05};
}}

.workspace-canvas,
.settings-canvas {{
  flex: 1;
  min-height: 0;
}}

.workspace-canvas {{
  position: relative;
  overflow: hidden;
}}

.settings-canvas {{
  padding: 16px;
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: contain;
}}

.workspace-viewport {{
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
}}

.workspace-viewport-overview {{
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
}}

.workspace-strip-canvas {{
  position: absolute;
  inset: 0 auto auto 0;
  transform-origin: top left;
}}

.workspace-surface-fallback-drop {{
  position: absolute;
  inset: 0;
  display: flex;
  align-items: flex-end;
  justify-content: flex-end;
  padding: 16px;
  border: 1px solid transparent;
  background: transparent;
  border-radius: 0;
}}

.workspace-surface-fallback-drop-active {{
  border-color: {accent_20};
  background: {accent_08};
  border-radius: 0;
}}

.workspace-surface-fallback-label {{
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-height: 28px;
  padding: 0 10px;
  border: 1px solid {border_10};
  background: {surface};
  color: {text_dim};
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  border-radius: 0;
}}

.workspace-viewport-overview .workspace-strip-canvas {{
  position: relative;
  inset: auto;
}}

.workspace-window-shell {{
  position: absolute;
  display: flex;
  flex-direction: column;
  background: {surface};
  border: {window_border_width}px solid {border_08};
  overflow: hidden;
  border-radius: 0;
  box-shadow: 0 2px 12px rgba(0,0,0,0.24);
}}

.workspace-window-shell-active {{
  border-color: {accent_24};
  box-shadow: 0 2px 14px rgba(0,0,0,0.32);
}}

.workspace-window-toolbar {{
  height: {window_toolbar_height}px;
  min-height: {window_toolbar_height}px;
  border-bottom: 1px solid {border_06};
  background: linear-gradient(180deg, {overlay_05} 0%, {overlay_03} 100%);
  position: relative;
  padding: 0 6px 0 8px;
  display: flex;
  align-items: center;
  gap: 8px;
  user-select: none;
  border-radius: 0;
}}

.workspace-window-toolbar-tabs {{
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 4px;
  overflow-x: auto;
  scrollbar-width: none;
}}

.workspace-window-toolbar-tabs::-webkit-scrollbar {{
  display: none;
}}

.workspace-window-toolbar-spacer {{
  flex: 1 1 auto;
  min-width: 24px;
  align-self: stretch;
  cursor: grab;
}}

.workspace-window-toolbar-spacer:active {{
  cursor: grabbing;
}}

.workspace-window-tab {{
  flex: 0 0 auto;
  min-width: 0;
  max-width: 280px;
  height: 24px;
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 0 3px 0 5px;
  background: transparent;
  border: 1px solid transparent;
  border-radius: 0;
  color: {text_subtle};
  overflow: hidden;
}}

.workspace-window-tab:hover {{
  background: {border_06};
  border-color: {border_10};
}}

.workspace-window-tab-active {{
  background: {overlay_16};
  border-color: {accent_20};
  color: {text_bright};
  box-shadow: inset 0 1px 0 rgba(255,255,255,0.04);
}}

.workspace-window-tab-drop-target {{
  border-color: {accent_24};
  background: {accent_12};
}}

.workspace-window-tab-dragging {{
  opacity: 0.34;
}}

.workspace-window-tab-button {{
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 0;
  background: transparent;
  padding: 0;
  color: inherit;
}}

.workspace-window-tab-copy {{
  min-width: 0;
  display: inline-flex;
  align-items: center;
}}

.workspace-window-tab-title {{
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
  font-weight: 600;
}}

.workspace-window-tab-kind-icon {{
  flex: 0 0 auto;
  opacity: 0.8;
}}

.workspace-window-tab-active .workspace-window-tab-kind-icon {{
  opacity: 1.0;
}}

.workspace-window-tab-close {{
  flex: 0 0 auto;
  width: 18px;
  height: 18px;
  border: 0;
  border-radius: 0;
  background: transparent;
  color: {text_dim};
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}}

.workspace-window-tab-close:hover {{
  background: {border_08};
  color: {text_bright};
}}

.workspace-window-tab-add {{
  flex: 0 0 auto;
  width: 22px;
  height: 22px;
  border: 1px solid {border_10};
  border-radius: 0;
  background: {overlay_05};
  color: {text_dim};
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}}

.workspace-window-tab-add:hover {{
  background: {overlay_16};
  color: {text_bright};
}}

.workspace-window-tab-add-active {{
  border-color: {accent_24};
  background: {accent_12};
  color: {text_bright};
}}

.workspace-window-tab-attention-waiting {{
  border-color: {waiting_18};
  background: {waiting_18};
  color: {waiting_text};
}}

.workspace-window-tab-attention-error {{
  border-color: {error_16};
  background: {error_16};
  color: {error_text};
}}

.workspace-window-tab-attention-completed {{
  border-color: {completed_16};
  background: {completed_16};
  color: {completed_text};
}}

.workspace-window-tab-attention-waiting .workspace-window-tab-kind-icon,
.workspace-window-tab-attention-error .workspace-window-tab-kind-icon,
.workspace-window-tab-attention-completed .workspace-window-tab-kind-icon {{
  opacity: 1.0;
}}

.workspace-window-body {{
  flex: 1;
  min-height: 0;
  padding: {window_body_padding}px;
  background: {surface};
}}

.split-container {{
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  display: flex;
  gap: {split_gap}px;
}}

.split-child {{
  display: flex;
  min-width: 0;
  min-height: 0;
}}

.pane-frame {{
  position: relative;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
}}

.pane-card {{
  position: relative;
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: {elevated};
  border: {pane_border_width}px solid {border_10};
  overflow: hidden;
  border-radius: 0;
}}

.pane-card-active {{
  border-color: {accent_24};
}}

.pane-card-drop-target {{
  border-color: {accent_24};
}}

@keyframes focus-flash {{
  0%   {{ opacity: 0; }}
  25%  {{ opacity: 1; }}
  50%  {{ opacity: 0; }}
  75%  {{ opacity: 1; }}
  100% {{ opacity: 0; }}
}}

.pane-flash-ring {{
  position: absolute;
  inset: 0;
  border: 1px solid {accent};
  pointer-events: none;
  opacity: 0;
  z-index: 10;
  border-radius: 0;
}}

.pane-flash-ring-active {{
  animation: focus-flash 0.9s ease-in-out;
}}

.pane-toolbar {{
  height: {pane_header_height}px;
  min-height: {pane_header_height}px;
  border-bottom: 1px solid {border_10};
  padding: 0 6px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  background: {surface};
  border-radius: 0;
}}

.pane-toolbar-meta,
.shortcut-label {{
  color: {text_subtle};
  font-size: 11px;
}}

.pane-toolbar-meta {{
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
}}

.pane-toolbar-meta-draggable {{
  cursor: grab;
  padding: 0 2px;
}}

.pane-toolbar-meta-draggable:active {{
  cursor: grabbing;
}}

.pane-toolbar-kind-icon {{
  flex: 0 0 auto;
  display: block;
}}

.pane-toolbar-title {{
  font-size: 12px;
  font-weight: 500;
  color: {text_bright};
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  min-width: 0;
}}

.pane-runtime-chip {{
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 2px 8px;
  border-radius: 999px;
  background: {overlay_12};
  border: 1px solid {border_10};
}}

.pane-runtime-copy {{
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1 1 auto;
}}

.pane-runtime-primary {{
  min-width: 0;
  font-size: 11px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: {text_bright};
  flex: 1 1 auto;
}}

.pane-runtime-badge {{
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  padding: 1px 5px;
  border-radius: 999px;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.03em;
  background: {border_08};
  color: {text_subtle};
}}

.pane-runtime-state {{
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  padding: 1px 6px;
  border-radius: 999px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.01em;
  background: {border_08};
  color: {text_bright};
}}

.pane-runtime-state-dismiss {{
  border: 0;
  cursor: pointer;
  pointer-events: auto;
}}

.pane-runtime-chip.runtime-state-working {{
  background: {busy_10};
  border-color: {busy_12};
}}

.pane-runtime-chip.runtime-state-waiting {{
  background: {waiting_10};
  border-color: {waiting_14};
}}

.pane-runtime-chip.runtime-state-completed {{
  background: {completed_10};
  border-color: {completed_12};
}}

.pane-runtime-chip.runtime-state-failed {{
  background: {error_10};
  border-color: {error_12};
}}

.pane-runtime-state.runtime-state-working {{
  background: {busy_16};
}}

.pane-runtime-state.runtime-state-waiting {{
  background: {waiting_18};
}}

.pane-runtime-state.runtime-state-completed {{
  background: {completed_16};
}}

.pane-runtime-state.runtime-state-failed {{
  background: {error_16};
}}

.pane-action-separator {{
  width: 1px;
  height: 14px;
  background: {border_10};
  margin: 0 2px;
}}

.pane-utility-icon {{
  display: block;
}}

.pane-action-cluster {{
  flex: 0 0 auto;
  display: flex;
  align-items: center;
  gap: 2px;
  opacity: 0;
  pointer-events: none;
  transition: opacity 0.14s ease-in-out;
}}

.pane-card-active .pane-action-cluster {{
  opacity: 1;
  pointer-events: auto;
}}

.pane-action-cluster-visible {{
  opacity: 1;
  pointer-events: auto;
}}

.pane-tabs {{
  height: {surface_tab_height}px;
  min-height: {surface_tab_height}px;
  border-bottom: 1px solid {border_10};
  padding: 0 4px;
  display: flex;
  align-items: center;
  background: {overlay_05};
}}

.pane-tabs-primary {{
  flex: 1 1 auto;
  min-width: 0;
  height: auto;
  min-height: 0;
  border-bottom: 0;
  padding: 0;
  background: transparent;
}}

.pane-tabs-inline {{
  flex: 0 0 auto;
  width: 100%;
}}

.surface-tabs {{
  flex: 1;
  min-width: 0;
  min-height: 0;
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 0;
  overflow-x: auto;
  background: transparent;
}}

.surface-tab {{
  display: inline-flex;
  align-items: center;
  gap: 5px;
  min-width: 0;
  max-width: 320px;
  height: 20px;
  border: 1px solid transparent;
  background: transparent;
  padding: 0 6px;
  color: {text_muted};
  white-space: nowrap;
  border-radius: 0;
  overflow: hidden;
}}

.surface-tab-focus {{
  min-width: 0;
  flex: 1 1 auto;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  border: 0;
  padding: 0;
  background: transparent;
  color: inherit;
  text-align: left;
  cursor: pointer;
}}

.surface-tab:hover {{
  background: {border_06};
  border-color: {border_10};
}}

.surface-tab-append-target {{
  min-width: 28px;
  justify-content: center;
  border-style: dashed;
  color: {text_dim};
}}

.surface-tab-draggable {{
  cursor: grab;
}}

.surface-tab-attention-waiting {{
  border-color: {waiting_18};
  background: {waiting_18};
  color: {waiting_text};
}}

.surface-tab-attention-waiting .surface-tab-primary,
.surface-tab-attention-waiting .surface-tab-kind-icon {{
  color: {waiting_text};
  opacity: 1.0;
}}

.surface-tab-attention-error {{
  border-color: {error_16};
  background: {error_16};
  color: {error_text};
}}

.surface-tab-attention-error .surface-tab-primary,
.surface-tab-attention-error .surface-tab-kind-icon {{
  color: {error_text};
  opacity: 1.0;
}}

.surface-tab-attention-completed {{
  border-color: {completed_16};
  background: {completed_16};
  color: {completed_text};
}}

.surface-tab-attention-completed .surface-tab-primary,
.surface-tab-attention-completed .surface-tab-kind-icon {{
  color: {completed_text};
  opacity: 1.0;
}}

.surface-tab-drop-target {{
  border-color: {accent_24};
  background: {accent_12};
}}

.surface-tab-dragging {{
  opacity: 0.34;
}}

.surface-tab-active {{
  background: {overlay_16};
  border-color: {accent_20};
  color: {text_bright};
  position: relative;
}}

.surface-tab-active::after {{
  content: '';
  position: absolute;
  bottom: -1px;
  left: 4px;
  right: 4px;
  height: 2px;
  background: {accent};
  border-radius: 1px;
}}

.surface-tab-attention-waiting.surface-tab-active {{
  border-color: {waiting};
}}

.surface-tab-attention-waiting.surface-tab-active::after {{
  background: {waiting};
}}

.surface-tab-attention-error.surface-tab-active {{
  border-color: {error};
}}

.surface-tab-attention-error.surface-tab-active::after {{
  background: {error};
}}

.surface-tab-attention-completed.surface-tab-active {{
  border-color: {completed};
}}

.surface-tab-attention-completed.surface-tab-active::after {{
  background: {completed};
}}

.surface-tab-copy {{
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1 1 auto;
}}

.surface-tab-primary {{
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: {text_subtle};
  font-size: 12px;
  flex: 1 1 auto;
}}

.surface-tab-runtime-badge {{
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  padding: 1px 5px;
  border-radius: 999px;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.03em;
  background: {border_08};
  color: {text_subtle};
}}

.surface-tab-active .surface-tab-primary {{
  color: {text_bright};
}}

.surface-tab-kind-icon {{
  flex: 0 0 auto;
  opacity: 0.7;
}}

.surface-tab-state {{
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  padding: 1px 5px;
  border-radius: 999px;
  font-size: 9px;
  font-weight: 700;
  letter-spacing: 0.01em;
  background: {border_08};
  color: {text_bright};
}}

.surface-tab-dismiss {{
  border: 0;
  cursor: pointer;
  transition: filter 0.14s ease-in-out;
}}

.surface-tab-dismiss:hover {{
  filter: brightness(1.08);
}}

.surface-tab-close {{
  flex: 0 0 auto;
  width: 18px;
  height: 18px;
  border: 0;
  background: transparent;
  color: {text_dim};
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}}

.surface-tab-close:hover {{
  background: {border_08};
  color: {text_bright};
}}

.surface-tab-add {{
  flex: 0 0 auto;
  width: 22px;
  height: 22px;
  border: 1px solid {border_10};
  background: {overlay_05};
  color: {text_dim};
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 0;
}}

.surface-tab-add:hover {{
  background: {overlay_16};
  color: {text_bright};
}}

.surface-tab-state.runtime-state-working {{
  background: {busy_16};
}}

.surface-tab-state.runtime-state-waiting {{
  background: {waiting_18};
}}

.surface-tab-state.runtime-state-completed {{
  background: {completed_16};
}}

.surface-tab-state.runtime-state-failed {{
  background: {error_16};
}}

.surface-tab-active .surface-tab-kind-icon {{
  opacity: 1.0;
}}

.drag-preview-shell {{
  position: fixed;
  inset: 0 auto auto 0;
  z-index: 9999;
  pointer-events: none;
}}

.drag-preview-card {{
  pointer-events: none;
  max-width: min(360px, calc(100vw - 32px));
  box-shadow: 0 12px 28px rgba(0, 0, 0, 0.38);
  opacity: 0.97;
  border-color: {accent_24};
  background: {surface_85};
  backdrop-filter: blur(10px) saturate(1.12);
}}

.drag-preview-window-tab {{
  min-width: 120px;
}}

.drag-preview-surface-tab {{
  min-width: 132px;
  padding-right: 10px;
}}

.drag-preview-copy {{
  min-width: 0;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: 1 1 auto;
}}

.drag-preview-title {{
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: {text_bright};
}}

.drag-preview-icon {{
  flex: 0 0 auto;
  opacity: 1.0;
}}

.pane-utility {{
  min-width: 22px;
  height: 20px;
  border: 0;
  padding: 0 5px;
  background: transparent;
  color: {text_dim};
  font-size: 10px;
  font-family: "IBM Plex Mono", ui-monospace, monospace;
  line-height: 1;
  border-radius: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}}

.pane-utility:hover {{
  background: {border_06};
  color: {text_bright};
  box-shadow: 0 1px 2px rgba(0,0,0,0.20);
}}

.workspace-window-drop-zone {{
  position: absolute;
  z-index: 12;
  opacity: 0;
  pointer-events: none;
  background: transparent;
  transition: opacity 0.12s ease-in-out, background 0.12s ease-in-out;
}}

.workspace-window-drop-zone-visible {{
  opacity: 0.45;
  pointer-events: auto;
  background: {accent_12};
}}

.workspace-window-drop-zone-active {{
  opacity: 1;
  pointer-events: auto;
  background: {accent_24};
}}

.workspace-window-drop-zone-left,
.workspace-window-drop-zone-right {{
  top: 10px;
  bottom: 10px;
  width: 6px;
}}

.workspace-window-drop-zone-left {{
  left: 0;
}}

.workspace-window-drop-zone-right {{
  right: 0;
}}

.workspace-window-drop-zone-top,
.workspace-window-drop-zone-bottom {{
  left: 12px;
  right: 12px;
  height: 6px;
}}

.workspace-window-drop-zone-top {{
  top: 0;
}}

.workspace-window-drop-zone-bottom {{
  bottom: 0;
}}

.pane-utility-close {{
  color: {error_text};
}}

.pane-utility-close:hover {{
  background: {error_10};
  color: {error};
}}

.live-pane-shell {{
  flex: 1 1 auto;
  width: 100%;
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  background: {elevated};
  border: {pane_border_width}px solid {border_10};
  overflow: hidden;
}}

.live-pane-shell-active {{
  border-color: {accent_24};
}}

.browser-toolbar {{
  height: {browser_toolbar_height}px;
  min-height: {browser_toolbar_height}px;
  border-bottom: 1px solid {border_06};
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  background: {surface};
}}

.browser-toolbar-button {{
  min-width: 30px;
  height: 26px;
  border: 1px solid {border_10};
  background: {overlay_05};
  color: {text_subtle};
  font-size: 11px;
  font-weight: 600;
  border-radius: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}}

.browser-toolbar-button:hover {{
  background: {overlay_16};
  color: {text_bright};
  box-shadow: 0 1px 2px rgba(0,0,0,0.20);
}}

.browser-toolbar-button:disabled {{
  opacity: 0.35;
  cursor: default;
  box-shadow: none;
}}

.browser-toolbar-button-primary {{
  border-color: {accent_24};
  color: {text_bright};
}}

.browser-address {{
  flex: 1;
  min-width: 0;
  height: 26px;
  border: 1px solid {border_10};
  padding: 0 14px;
  background: {overlay_05};
  color: {text_bright};
  font-size: 12px;
  border-radius: 0;
}}

.browser-address:focus {{
  outline: none;
  border-color: {accent_24};
  box-shadow: 0 0 0 1px {accent_20};
}}

.browser-toolbar-badge {{
  height: 26px;
  border: 1px solid {accent_24};
  padding: 0 10px;
  display: flex;
  align-items: center;
  gap: 6px;
  background: {accent_12};
  color: {text_bright};
  font-size: 11px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.08em;
}}

.pane-body {{
  flex: 1;
  min-height: 0;
  padding: 0;
  background: {border_03};
  position: relative;
  overflow: hidden;
}}

.pane-drop-overlay {{
  position: absolute;
  inset: 0;
  z-index: 8;
  pointer-events: none;
}}

.pane-drop-target {{
  position: absolute;
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid {accent_24};
  background: {accent_12};
  color: {text_bright};
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  pointer-events: auto;
  border-radius: 0;
}}

.pane-drop-target-active {{
  background: {accent_20};
  border-style: solid;
}}

.pane-drop-target-center {{
  inset: 18%;
}}

.pane-drop-target-edge {{
  z-index: 1;
}}

.pane-drop-target-left,
.pane-drop-target-right {{
  top: 10px;
  bottom: 10px;
  width: 56px;
}}

.pane-drop-target-left {{
  left: 10px;
}}

.pane-drop-target-right {{
  right: 10px;
}}

.pane-drop-target-top,
.pane-drop-target-bottom {{
  left: 10px;
  right: 10px;
  height: 48px;
}}

.pane-drop-target-top {{
  top: 10px;
}}

.pane-drop-target-bottom {{
  bottom: 10px;
}}

.surface-backdrop {{
  width: 100%;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 12px;
  border: 1px solid {border_06};
  padding: 12px;
  background: linear-gradient(180deg, {overlay_05} 0%, {overlay_03} 100%);
  border-radius: 0;
}}

.workspace-main-overview .workspace-window-shell {{
  box-shadow: none;
}}

.workspace-main-overview .workspace-window-toolbar {{
  min-height: {window_toolbar_height}px;
  padding: 0 6px;
  background: {surface_85};
}}

.workspace-main-overview .workspace-window-grip {{
  width: 28px;
}}

.workspace-main-overview .workspace-window-body {{
  padding: 8px;
  background: {border_03};
}}

.workspace-main-overview .workspace-window-flags,
.workspace-main-overview .pane-action-cluster,
.workspace-main-overview .browser-toolbar,
.workspace-main-overview .surface-backdrop-note,
.workspace-main-overview .surface-chip {{
  display: none;
}}

.workspace-main-overview .split-container {{
  gap: 8px;
}}

.workspace-main-overview .pane-toolbar,
.workspace-main-overview .pane-tabs {{
  min-height: 28px;
  padding: 0 8px;
}}

.workspace-main-overview .surface-tab {{
  height: 22px;
  max-width: 180px;
  padding: 0 6px;
}}

.workspace-main-overview .surface-tab-primary {{
  font-size: 10px;
}}

.workspace-main-overview .pane-body {{
  padding: 10px;
}}

.workspace-main-overview .pane-drop-overlay {{
  display: none;
}}

.workspace-main-overview .surface-backdrop {{
  gap: 8px;
  padding: 10px;
  border-style: solid;
  background:
    linear-gradient(180deg, {overlay_16} 0%, {overlay_03} 100%),
    {border_03};
}}

.surface-backdrop-copy {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}

.surface-backdrop-icon {{
  color: {text_dim};
  opacity: 0.6;
  margin-bottom: 4px;
}}

.browser-toolbar-icon {{
  display: block;
}}

.surface-backdrop-title {{
  color: {text_bright};
  font-size: 22px;
  font-weight: 600;
}}

.surface-backdrop-note {{
  max-width: 70ch;
  color: {text_subtle};
  font-size: 13px;
  line-height: 1.45;
}}

.surface-meta {{
  flex-wrap: wrap;
}}

.surface-chip {{
  padding: 4px 8px;
  color: {text_muted};
  font-size: 11px;
  border-radius: 0;
}}

.shortcut-pill {{
  padding: 4px 8px;
  color: {text_muted};
  font-size: 11px;
  border-radius: 0;
  background: {border_06};
  border: 1px solid {border_10};
  box-shadow: 0 1px 0 {border_08};
  font-family: "IBM Plex Mono", ui-monospace, monospace;
}}

.shortcut-pill-muted {{
  opacity: 0.72;
}}

.status-dot {{
  font-size: 10px;
}}

.status-dot-normal {{
  color: {text_dim};
}}

.status-dot-busy {{
  color: {busy};
}}

.status-dot-completed {{
  color: {completed};
}}

.status-dot-waiting {{
  color: {waiting};
}}

.status-dot-error {{
  color: {error};
}}

.empty-state {{
  border: 1px solid {border_10};
  padding: 12px;
  color: {text_dim};
  font-size: 12px;
  border-radius: 0;
}}

.activity-item {{
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  border-radius: 0;
}}

.activity-item-button {{
  flex: 1 1 auto;
  width: 100%;
  border: 0;
  padding: 0;
  background: transparent;
  text-align: left;
  cursor: pointer;
}}

.activity-item-row {{
  display: flex;
  align-items: flex-start;
  gap: 6px;
}}

.activity-item-button:hover .activity-item {{
  background: {border_05};
}}

.activity-item-dismiss {{
  min-width: 0;
  height: 18px;
  border: 0;
  margin-top: 4px;
  background: transparent;
  color: {text_dim};
  padding: 0 6px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 0;
  cursor: pointer;
  transition: background 0.14s ease-in-out, color 0.14s ease-in-out;
}}

.activity-item-dismiss:hover {{
  background: {border_08};
  color: {text_bright};
}}

.activity-item-dismiss-label {{
  font-weight: 700;
}}

.notification-header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.notification-counts {{
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
}}

.notification-jump-button {{
  border: 1px solid {border_08};
  background: transparent;
  color: {text_bright};
  font-size: 10px;
  padding: 3px 7px;
  border-radius: 0;
}}

.notification-count-pill {{
  font-size: 10px;
  font-weight: 600;
  color: {text_dim};
  padding: 2px 6px;
  background: {border_06};
  border-radius: 0;
}}

.notification-count-unread {{
  background: {accent_14};
  color: {text_bright};
}}

.vcs-refresh-button {{
  display: inline-flex;
  align-items: center;
  gap: 6px;
}}

.vcs-panel-error {{
  font-size: 11px;
  color: {error};
  padding: 8px 10px;
  background: {error_10};
  border: 1px solid {error_16};
  border-radius: 0;
}}

.vcs-panel-summary {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}

.vcs-panel-repo-row {{
  display: flex;
  align-items: center;
  gap: 8px;
}}

.vcs-actions,
.vcs-files,
.vcs-diff {{
  border-top: 1px solid {border_06};
  padding-top: 10px;
}}

.vcs-action-row,
.vcs-ref-list {{
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}}

.vcs-inline-form {{
  display: flex;
  gap: 6px;
  align-items: center;
}}

.vcs-input {{
  flex: 1 1 auto;
  min-width: 0;
}}

.vcs-ref-chip {{
  min-height: 24px;
  padding: 0 8px;
  font-size: 10px;
}}

.vcs-ref-chip-active {{
  background: {accent_14};
  border-color: {accent_24};
  color: {text_bright};
}}

.vcs-file-list {{
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 220px;
  overflow-y: auto;
}}

.vcs-file-row {{
  width: 100%;
  border: 1px solid {border_06};
  background: {border_03};
  color: {text_bright};
  padding: 8px 10px;
  border-radius: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  text-align: left;
}}

.vcs-file-row:hover,
.vcs-file-row-active {{
  background: {accent_08};
  border-color: {accent_24};
}}

.vcs-diff-preview {{
  margin: 0;
  padding: 10px;
  background: {border_03};
  border: 1px solid {border_06};
  border-radius: 0;
  white-space: pre-wrap;
  overflow: auto;
  max-height: 280px;
  font-size: 11px;
  line-height: 1.5;
  color: {text_subtle};
  font-family: "IBM Plex Mono", ui-monospace, monospace;
}}

.vcs-pr-link {{
  text-decoration: none;
  padding: 8px 10px;
  background: {border_03};
  border: 1px solid {border_06};
  border-radius: 0;
}}

.vcs-pr-link:hover {{
  background: {accent_08};
  border-color: {accent_24};
}}

.agent-session-list {{
  display: flex;
  flex-direction: column;
  gap: 4px;
}}

.attention-section {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}

.attention-section-title {{
  font-size: 10px;
  font-weight: 600;
  color: {text_dim};
  letter-spacing: 0.08em;
  text-transform: uppercase;
}}

.attention-status-text {{
  font-size: 12px;
  color: {text_bright};
}}

.notification-timeline {{
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 4px;
}}

.notification-row-button {{
  display: block;
  width: 100%;
  border: 0;
  padding: 0;
  background: transparent;
  text-align: left;
  cursor: pointer;
}}

.notification-row {{
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px;
  background: {border_03};
  transition: background 0.14s ease-in-out;
  border-radius: 0;
}}

.workspace-log-list {{
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 180px;
  overflow-y: auto;
}}

.workspace-log-entry {{
  background: {border_03};
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
  border-radius: 0;
}}

.workspace-log-entry-header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.workspace-log-source,
.workspace-log-time {{
  font-size: 10px;
  color: {text_dim};
}}

.workspace-log-message {{
  font-size: 11px;
  color: {text_bright};
  line-height: 1.35;
}}

.notification-row-button:hover .notification-row {{
  background: {border_06};
}}

.notification-dot {{
  flex: 0 0 auto;
  width: 8px;
  height: 8px;
  margin-top: 4px;
  border-radius: 9999px;
}}

.notification-dot-unread {{
  background: {accent};
}}

.notification-dot-read {{
  background: transparent;
  border: 1px solid {accent_20};
}}

.notification-row-content {{
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}}

.notification-row-header {{
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
}}

.notification-title {{
  font-weight: 600;
  font-size: 12.5px;
  color: {text_bright};
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}}

.notification-body {{
  color: {text_subtle};
  font-size: 11px;
  line-height: 1.4;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}}

.notification-timestamp {{
  flex: 0 0 auto;
  font-size: 10px;
  color: {text_dim};
  white-space: nowrap;
}}

.notification-row-footer {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}}

.notification-source {{
  font-size: 10px;
  color: {text_dim};
}}

.notification-clear {{
  width: 16px;
  height: 16px;
  border: 0;
  background: transparent;
  color: {text_dim};
  font-size: 13px;
  line-height: 1;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 0;
  transition: background 0.14s ease-in-out, color 0.14s ease-in-out;
}}

.notification-clear-icon {{
  display: block;
}}

.notification-empty-icon {{
  color: {text_dim};
  opacity: 0.4;
}}

.agent-kind-icon {{
  flex: 0 0 auto;
  display: block;
}}

.notification-clear:hover {{
  background: {error_16};
  color: {error};
}}

.notification-empty {{
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 32px 16px;
}}

.notification-empty-title {{
  font-weight: 600;
  font-size: 13px;
  color: {text_muted};
}}

.notification-empty-subtitle {{
  font-size: 11px;
  color: {text_dim};
  text-align: center;
  line-height: 1.4;
}}

.settings-grid {{
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 16px;
}}

.settings-card-span {{
  grid-column: 1 / -1;
}}

.theme-grid,
.preset-grid {{
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}}

.theme-card,
.preset-card {{
  border: 1px solid {border_08};
  padding: 10px;
  border-radius: 0;
}}

.theme-card:hover,
.preset-card:hover {{
  background: {border_05};
  border-color: {border_12};
}}

.theme-card-active,
.preset-card-active {{
  background: {accent_12};
  border-color: {accent_24};
}}

.shortcut-groups {{
  display: flex;
  flex-direction: column;
  gap: 14px;
}}

.shortcut-group {{
  display: flex;
  flex-direction: column;
  gap: 8px;
}}

.shortcut-list {{
  display: flex;
  flex-direction: column;
  gap: 8px;
}}

.shortcut-row {{
  align-items: flex-start;
  border-top: 1px solid {border_06};
  padding-top: 8px;
}}

.shortcut-row:first-child {{
  border-top: 0;
  padding-top: 0;
}}

.shortcut-accelerators {{
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
  max-width: 40%;
}}

@media (max-width: 1180px) {{
  .app-shell {{
    grid-template-columns: 184px minmax(0, 1fr);
  }}

  .attention-panel {{
    display: none;
  }}
}}
"#,
        base = p.base.to_hex(),
        surface = p.surface.to_hex(),
        surface_85 = rgba(p.surface, 0.85),
        elevated = p.elevated.to_hex(),
        overlay_03 = rgba(p.overlay, 0.03),
        overlay_05 = rgba(p.overlay, 0.05),
        overlay_12 = rgba(p.overlay, 0.12),
        overlay_16 = rgba(p.overlay, 0.16),
        text = p.text.to_hex(),
        text_bright = p.text_bright.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        text_dim = p.text_dim.to_hex(),
        border_03 = rgba(p.border, 0.03),
        border_05 = rgba(p.border, 0.05),
        border_06 = rgba(p.border, 0.06),
        border_08 = rgba(p.border, 0.08),
        border_10 = rgba(p.border, 0.10),
        border_12 = rgba(p.border, 0.12),
        accent = p.accent.to_hex(),
        accent_08 = rgba(p.accent, 0.08),
        accent_12 = rgba(p.accent, 0.12),
        accent_14 = rgba(p.accent, 0.14),
        accent_20 = rgba(p.accent, 0.20),
        accent_24 = rgba(p.accent, 0.24),
        busy = p.busy.to_hex(),
        busy_10 = rgba(p.busy, 0.10),
        busy_12 = rgba(p.busy, 0.12),
        busy_16 = rgba(p.busy, 0.16),
        busy_text = p.busy_text.to_hex(),
        completed = p.completed.to_hex(),
        completed_10 = rgba(p.completed, 0.10),
        completed_12 = rgba(p.completed, 0.12),
        completed_16 = rgba(p.completed, 0.16),
        completed_text = p.completed_text.to_hex(),
        waiting = p.waiting.to_hex(),
        waiting_10 = rgba(p.waiting, 0.10),
        waiting_14 = rgba(p.waiting, 0.14),
        waiting_18 = rgba(p.waiting, 0.18),
        waiting_text = p.waiting_text.to_hex(),
        error = p.error.to_hex(),
        error_10 = rgba(p.error, 0.10),
        error_12 = rgba(p.error, 0.12),
        error_16 = rgba(p.error, 0.16),
        error_text = p.error_text.to_hex(),
    );
    css
}
