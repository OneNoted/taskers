use std::fmt::Write as _;

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

pub fn generate_css(p: &ThemePalette) -> String {
    let mut css = String::with_capacity(18_000);
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
}}

.app-shell {{
  width: 100vw;
  height: 100vh;
  background: {base};
  display: grid;
  grid-template-columns: 248px minmax(0, 1fr) 312px;
  overflow: hidden;
}}

.workspace-sidebar,
.attention-panel {{
  background: {surface_85};
  backdrop-filter: blur(20px);
  -webkit-backdrop-filter: blur(20px);
  display: flex;
  flex-direction: column;
  min-height: 0;
}}

.workspace-sidebar {{
  border-right: 1px solid {border_04};
  padding: 8px;
  gap: 10px;
}}

.attention-panel {{
  border-left: 1px solid {border_04};
  padding: 10px 12px;
  gap: 8px;
}}

.sidebar-brand {{
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}}

.sidebar-brand h1 {{
  margin: 0;
  font-size: 24px;
  line-height: 1;
  color: {text_bright};
}}

.sidebar-heading {{
  font-weight: 600;
  font-size: 11px;
  color: {text_dim};
  letter-spacing: 0.10em;
  text-transform: uppercase;
}}

.sidebar-nav,
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

.sidebar-nav-button,
.workspace-button,
.theme-card,
.preset-card {{
  width: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
}}

.sidebar-nav-button {{
  border-radius: 7px;
  padding: 8px 10px;
  color: {text_subtle};
}}

.sidebar-nav-button:hover,
.sidebar-nav-button-active {{
  background: {border_06};
  color: {text_bright};
}}

.sidebar-section-header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 0 6px;
}}

.workspace-add {{
  background: transparent;
  color: {text_dim};
  border: 1px solid {border_10};
  border-radius: 999px;
  min-width: 24px;
  min-height: 24px;
  padding: 0;
  font-size: 16px;
}}

.workspace-add:hover {{
  background: {waiting_10};
  color: {waiting_text};
  border-color: {waiting_18};
}}

.workspace-tab {{
  position: relative;
  padding: 8px 10px 8px 14px;
  border-radius: 6px;
  border: 1px solid transparent;
  display: flex;
  align-items: stretch;
  gap: 0;
  transition: background 0.14s ease-in-out, border-color 0.14s ease-in-out;
}}

.workspace-button:hover .workspace-tab {{
  background: {border_04};
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
  top: 5px;
  bottom: 5px;
  width: 3px;
  border-radius: 1.5px;
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
  border-radius: 4px;
  background: transparent;
  color: {text_dim};
  font-size: 13px;
  line-height: 1;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  visibility: hidden;
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
  border-radius: 999px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 9px;
  font-weight: 700;
  background: var(--workspace-accent, {accent});
  color: {base};
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

.workspace-branch-row {{
  color: {text_muted};
  font-size: 10px;
  font-family: "IBM Plex Mono", ui-monospace, monospace;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}}

.workspace-ports-row {{
  color: {text_dim};
  font-size: 10px;
  font-family: "IBM Plex Mono", ui-monospace, monospace;
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
  border-radius: 9px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}}

.runtime-row,
.attention-summary {{
  display: flex;
  flex-direction: column;
  gap: 4px;
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
  border-radius: 999px;
  padding: 4px 8px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
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
}}

.workspace-main-overview .workspace-canvas {{
  background: {border_03};
}}

.workspace-header {{
  height: 48px;
  min-height: 48px;
  border-bottom: 1px solid {border_07};
  padding: 0 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: {base};
}}

.workspace-header-group {{
  display: flex;
  align-items: center;
  gap: 2px;
  background: {border_04};
  border-radius: 8px;
  padding: 2px;
}}

.workspace-header-group .workspace-header-action {{
  border: 0;
  min-height: 26px;
  padding: 0 8px;
}}

.workspace-header-divider {{
  width: 1px;
  height: 16px;
  background: {border_10};
  margin: 0 4px;
}}

.workspace-header-main,
.workspace-header-actions,
.pane-header-main,
.pane-action-cluster,
.surface-meta,
.activity-header,
.activity-item-shell,
.shortcut-row {{
  display: flex;
  align-items: center;
  gap: 8px;
}}

.workspace-header-main,
.shortcut-row {{
  justify-content: space-between;
}}

.workspace-header-title-btn {{
  background: transparent;
  border: 0;
  border-radius: 7px;
  color: inherit;
  padding: 6px 8px;
  text-align: left;
}}

.workspace-header-title-btn:hover {{
  background: {border_06};
}}

.workspace-header-label {{
  display: block;
  font-weight: 600;
  font-size: 14px;
  color: {text_bright};
}}

.workspace-header-meta {{
  display: block;
  font-size: 12px;
  color: {text_dim};
}}

.workspace-header-action,
.pane-action,
.activity-action,
.shortcut-pill {{
  border: 1px solid {border_10};
  border-radius: 999px;
  background: transparent;
}}

.workspace-header-action,
.pane-action,
.activity-action {{
  min-height: 28px;
  padding: 0 10px;
  color: {text_subtle};
}}

.workspace-header-action:hover,
.pane-action:hover {{
  background: {border_06};
  color: {text_bright};
}}

.activity-action-passive {{
  display: inline-flex;
  align-items: center;
  color: {text_dim};
  background: {border_04};
}}

.workspace-header-action-active {{
  background: {accent_14};
  color: {text_bright};
}}

.workspace-header-action-primary {{
  background: {accent_14};
  color: {text_bright};
  border-color: {accent_24};
}}

.workspace-header-action-primary:hover {{
  background: {accent_22};
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
}}

.workspace-viewport {{
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
}}

.workspace-strip-canvas {{
  position: absolute;
  inset: 0 auto auto 0;
  transform-origin: top left;
}}

.workspace-window-shell {{
  position: absolute;
  display: flex;
  flex-direction: column;
  border-radius: 10px;
  background: {elevated};
  border: 1px solid {border_08};
  overflow: hidden;
  box-shadow: 0 18px 42px {overlay_16};
}}

.workspace-window-shell-active {{
  border-color: {accent_24};
  box-shadow: 0 18px 42px {overlay_16}, 0 0 0 1px {accent_24};
}}

.workspace-window-shell-state-busy {{
  box-shadow: 0 18px 42px {overlay_16}, inset 0 0 0 1px {busy_10};
}}

.workspace-window-shell-state-completed {{
  box-shadow: 0 18px 42px {overlay_16}, inset 0 0 0 1px {completed_10};
}}

.workspace-window-shell-state-waiting {{
  box-shadow: 0 18px 42px {overlay_16}, inset 0 0 0 1px {waiting_10};
}}

.workspace-window-shell-state-error {{
  box-shadow: 0 18px 42px {overlay_16}, inset 0 0 0 1px {error_10};
}}

.workspace-window-toolbar {{
  min-height: 38px;
  border-bottom: 1px solid {border_07};
  background: {surface};
  padding: 0 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.workspace-window-title {{
  background: transparent;
  border: 0;
  color: inherit;
  padding: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  text-align: left;
}}

.workspace-window-title:hover {{
  color: {text_bright};
}}

.workspace-window-flags {{
  display: flex;
  align-items: center;
  gap: 6px;
}}

.workspace-window-body {{
  flex: 1;
  min-height: 0;
  padding: 10px;
  background: {border_02};
}}

.split-container {{
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  display: flex;
  gap: 12px;
}}

.split-child {{
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
  border: 1px solid {border_07};
  border-radius: 8px;
  overflow: hidden;
}}

.pane-card-active {{
  border-color: {accent_20};
}}

.pane-card-state-busy {{
  box-shadow: inset 0 0 0 1px {busy_10};
}}

.pane-card-state-completed {{
  box-shadow: inset 0 0 0 1px {completed_10};
}}

.pane-card-state-waiting {{
  box-shadow: inset 0 0 0 1px {waiting_12};
}}

.pane-card-state-error {{
  box-shadow: inset 0 0 0 1px {error_10};
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
  inset: 6px;
  border-radius: 10px;
  border: 3px solid {accent};
  box-shadow: 0 0 12px {accent_20};
  pointer-events: none;
  opacity: 0;
  z-index: 10;
}}

.pane-flash-ring-active {{
  animation: focus-flash 0.9s ease-in-out;
}}

.pane-header {{
  min-height: 38px;
  border-bottom: 1px solid {border_07};
  padding: 0 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.pane-title-stack {{
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}}

.pane-title {{
  color: {text_bright};
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}}

.pane-meta,
.surface-tab-label,
.shortcut-label {{
  color: {text_subtle};
  font-size: 11px;
}}

.pane-action-tab {{
  border-color: {accent_20};
}}

.pane-window-action {{
  border-color: {action_window_22};
}}

.pane-split-action {{
  border-color: {action_split_22};
}}

.pane-close-action {{
  border-color: {error_18};
}}

.pane-close-action:hover {{
  background: {error_10};
  color: {error};
}}

.surface-tabs {{
  min-height: 34px;
  border-bottom: 1px solid {border_06};
  display: flex;
  align-items: stretch;
  gap: 6px;
  padding: 6px 8px;
  overflow-x: auto;
  background: {border_03};
}}

.surface-tab {{
  display: inline-flex;
  align-items: center;
  gap: 6px;
  border: 1px solid transparent;
  border-radius: 999px;
  background: transparent;
  padding: 5px 9px;
  color: {text_muted};
  white-space: nowrap;
}}

.surface-tab:hover {{
  background: {border_06};
  border-color: {border_10};
}}

.surface-tab-active {{
  background: {accent_14};
  border-color: {accent_24};
  color: {text_bright};
}}

.surface-tab-state-busy {{
  box-shadow: inset 0 0 0 1px {busy_10};
}}

.surface-tab-state-completed {{
  box-shadow: inset 0 0 0 1px {completed_10};
}}

.surface-tab-state-waiting {{
  box-shadow: inset 0 0 0 1px {waiting_10};
}}

.surface-tab-state-error {{
  box-shadow: inset 0 0 0 1px {error_10};
}}

.browser-toolbar {{
  min-height: 42px;
  border-bottom: 1px solid {border_06};
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: {overlay_05};
}}

.browser-toolbar-button {{
  min-width: 34px;
  height: 28px;
  border-radius: 8px;
  border: 1px solid {border_10};
  background: {overlay_05};
  color: {text_subtle};
  font-size: 11px;
  font-weight: 600;
}}

.browser-toolbar-button:hover {{
  background: {overlay_16};
  color: {text_bright};
}}

.browser-toolbar-button-primary {{
  border-color: {accent_24};
  color: {text_bright};
}}

.browser-address {{
  flex: 1;
  min-width: 0;
  height: 28px;
  border-radius: 8px;
  border: 1px solid {border_10};
  padding: 0 10px;
  background: {overlay_05};
  color: {text_bright};
  font-size: 12px;
}}

.browser-address:focus {{
  outline: none;
  border-color: {accent_24};
  box-shadow: 0 0 0 1px {accent_20};
}}

.pane-body {{
  flex: 1;
  min-height: 0;
  padding: 18px;
  background: {border_02};
}}

.surface-backdrop {{
  width: 100%;
  height: 100%;
  min-height: 0;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  gap: 14px;
  border: 1px dashed {border_12};
  border-radius: 8px;
  padding: 18px;
  background:
    linear-gradient(180deg, {overlay_16} 0%, {overlay_05} 100%),
    {overlay_03};
}}

.surface-backdrop-copy {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}

.surface-backdrop-eyebrow {{
  font-size: 11px;
  font-weight: 700;
  letter-spacing: 0.10em;
  text-transform: uppercase;
  color: {text_dim};
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

.surface-chip,
.shortcut-pill {{
  padding: 4px 8px;
  color: {text_muted};
  font-size: 11px;
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
  border: 1px dashed {border_10};
  border-radius: 8px;
  padding: 12px;
  color: {text_dim};
  font-size: 12px;
}}

.activity-item {{
  border-radius: 8px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
}}

.activity-item-button {{
  width: 100%;
  border: 0;
  padding: 0;
  background: transparent;
  text-align: left;
}}

.activity-item-button:hover .activity-item {{
  background: {border_04};
}}

.notification-header {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}}

.notification-counts {{
  display: flex;
  gap: 6px;
}}

.notification-count-pill {{
  font-size: 10px;
  font-weight: 600;
  color: {text_dim};
  padding: 2px 6px;
  border-radius: 999px;
  background: {border_06};
}}

.notification-count-unread {{
  background: {accent_14};
  color: {text_bright};
}}

.agent-session-list {{
  display: flex;
  flex-direction: column;
  gap: 4px;
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
  width: 100%;
  border: 0;
  padding: 0;
  background: transparent;
  text-align: left;
}}

.notification-row {{
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 10px;
  border-radius: 8px;
  background: {border_03};
  transition: background 0.14s ease-in-out;
}}

.notification-row-button:hover .notification-row {{
  background: {border_06};
}}

.notification-dot {{
  flex: 0 0 auto;
  width: 8px;
  height: 8px;
  border-radius: 999px;
  margin-top: 4px;
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
  border-radius: 4px;
  background: transparent;
  color: {text_dim};
  font-size: 13px;
  line-height: 1;
  padding: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background 0.14s ease-in-out, color 0.14s ease-in-out;
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

.notification-empty-icon {{
  font-size: 28px;
  color: {text_dim};
  opacity: 0.5;
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
  border-radius: 8px;
  padding: 10px;
}}

.theme-card:hover,
.preset-card:hover {{
  background: {border_04};
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
    grid-template-columns: 228px minmax(0, 1fr);
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
        overlay_16 = rgba(p.overlay, 0.16),
        text = p.text.to_hex(),
        text_bright = p.text_bright.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        text_dim = p.text_dim.to_hex(),
        border_02 = rgba(p.border, 0.02),
        border_03 = rgba(p.border, 0.03),
        border_04 = rgba(p.border, 0.04),
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        border_08 = rgba(p.border, 0.08),
        border_10 = rgba(p.border, 0.10),
        border_12 = rgba(p.border, 0.12),
        accent = p.accent.to_hex(),
        accent_08 = rgba(p.accent, 0.08),
        accent_12 = rgba(p.accent, 0.12),
        accent_14 = rgba(p.accent, 0.14),
        accent_20 = rgba(p.accent, 0.20),
        accent_22 = rgba(p.accent, 0.22),
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
        waiting_12 = rgba(p.waiting, 0.12),
        waiting_14 = rgba(p.waiting, 0.14),
        waiting_18 = rgba(p.waiting, 0.18),


        waiting_text = p.waiting_text.to_hex(),
        error = p.error.to_hex(),
        error_10 = rgba(p.error, 0.10),
        error_12 = rgba(p.error, 0.12),
        error_16 = rgba(p.error, 0.16),
        error_18 = rgba(p.error, 0.18),


        error_text = p.error_text.to_hex(),
        action_window_22 = rgba(p.action_window, 0.22),
        action_split_22 = rgba(p.action_split, 0.22),
    );
    css
}
