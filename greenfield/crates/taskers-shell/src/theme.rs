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

fn rgba(color: Color, alpha: f32) -> String {
    format!("rgba({},{},{},{alpha:.2})", color.r, color.g, color.b)
}

pub fn generate_css(p: &ThemePalette) -> String {
    let mut css = String::with_capacity(8192);
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
* {{ box-sizing: border-box; }}
button {{ font: inherit; }}
.app-shell {{
  width: 100vw;
  height: 100vh;
  background: linear-gradient(180deg, {base} 0%, {surface} 100%);
  display: flex;
  overflow: hidden;
}}
.workspace-sidebar {{
  width: 248px;
  flex: 0 0 248px;
  background: {surface};
  border-right: 1px solid {border_04};
  padding: 10px 8px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}}
.sidebar-heading {{
  font-weight: 600;
  font-size: 11px;
  color: {text_dim};
  letter-spacing: 0.10em;
  text-transform: uppercase;
}}
.sidebar-brand {{
  padding: 6px 8px 2px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}}
.sidebar-brand h1 {{
  margin: 0;
  font-size: 26px;
  line-height: 1;
  color: {text_bright};
}}
.workspace-list {{
  display: flex;
  flex-direction: column;
  gap: 6px;
}}
.workspace-button {{
  padding: 0;
  border: 0;
  background: transparent;
  text-align: left;
}}
.workspace-item {{
  padding: 8px 9px;
  border-radius: 8px;
  border: 1px solid transparent;
  background: transparent;
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 8px;
  transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
}}
.workspace-button:hover .workspace-item {{
  background: {border_04};
  border-color: {border_10};
}}
.workspace-item-active {{
  background: {border_05};
  border-color: {border_10};
}}
.workspace-label {{
  font-weight: 600;
  font-size: 13px;
  color: {text_bright};
}}
.workspace-preview {{
  color: {text_subtle};
  font-size: 12px;
  line-height: 1.35;
}}
.workspace-meta {{
  color: {text_dim};
  font-size: 11px;
}}
.workspace-status-badge {{
  background: {accent_14};
  color: {busy_text};
  border-radius: 999px;
  padding: 2px 6px;
  min-width: 18px;
  text-align: center;
  font-size: 11px;
  font-weight: 700;
}}
.runtime-card {{
  background: transparent;
  border: 1px solid {border_06};
  border-radius: 8px;
  padding: 9px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}}
.runtime-status-row {{
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
}}
.status-pill {{
  border-radius: 999px;
  padding: 3px 8px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.06em;
  text-transform: uppercase;
}}
.status-pill-ready {{
  background: {completed_16};
  color: {completed_text};
}}
.status-pill-fallback {{
  background: {waiting_18};
  color: {waiting_text};
}}
.status-pill-unavailable {{
  background: {error_16};
  color: {error_text};
}}
.status-copy {{
  color: {text_subtle};
  font-size: 12px;
  line-height: 1.4;
}}
.workspace-main {{
  min-width: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}}
.workspace-header {{
  height: 52px;
  min-height: 52px;
  border-bottom: 1px solid {border_07};
  padding: 0 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  background: {base};
}}
.workspace-header-title-btn {{
  background: transparent;
  border: 0;
  border-radius: 6px;
  color: {text_bright};
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
.workspace-header-actions {{
  display: flex;
  align-items: center;
  gap: 6px;
}}
.workspace-header-action {{
  background: transparent;
  border: 0;
  border-radius: 6px;
  min-width: 28px;
  min-height: 28px;
  color: {text_faint};
  padding: 0 10px;
}}
.workspace-header-action:hover {{
  background: {border_06};
  color: {text_muted};
}}
.workspace-header-action-primary {{
  background: {accent_14};
  color: {text_bright};
}}
.workspace-header-action-primary:hover {{
  background: {accent_22};
}}
.workspace-canvas {{
  flex: 1;
  min-height: 0;
  padding: 14px;
}}
.split-container {{
  width: 100%;
  height: 100%;
  display: flex;
  gap: 12px;
  min-width: 0;
  min-height: 0;
}}
.split-child {{
  min-width: 0;
  min-height: 0;
}}
.pane-card {{
  width: 100%;
  height: 100%;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  background: {elevated};
  border: 1px solid {border_07};
  border-radius: 8px;
  overflow: hidden;
}}
.pane-card-active {{
  border-color: {accent_20};
}}
.pane-header {{
  background: {border_02};
  border-bottom: 1px solid {border_05};
  padding: 5px 8px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  transition: background 160ms ease-in-out;
}}
.pane-card:hover .pane-header {{
  background: {border_04};
}}
.pane-card-active .pane-header {{
  background: {accent_06};
  border-bottom-color: {accent_15};
}}
.pane-header-main {{
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
}}
.status-dot {{
  font-size: 12px;
  line-height: 1;
}}
.status-dot-normal {{ color: {text_faint}; }}
.status-dot-busy {{ color: {busy}; }}
.status-dot-completed {{ color: {completed}; }}
.status-dot-waiting {{ color: {waiting}; }}
.status-dot-error {{ color: {error}; }}
.pane-title-stack {{
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}}
.pane-title {{
  font-weight: 500;
  color: {text_muted};
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}}
.pane-card-active .pane-title {{
  color: {text};
}}
.pane-meta {{
  color: {text_faint};
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}}
.pane-action-cluster {{
  display: flex;
  align-items: center;
  gap: 4px;
  background: {border_04};
  border: 1px solid {border_05};
  border-radius: 999px;
  padding: 2px;
}}
.pane-card-active .pane-action-cluster {{
  background: {accent_06};
  border-color: {accent_12};
}}
.pane-action {{
  background: transparent;
  border: 1px solid transparent;
  border-radius: 999px;
  min-width: 24px;
  min-height: 22px;
  padding: 0 8px;
  color: {text_faint};
}}
.pane-action:hover {{
  background: {accent_12};
  border-color: {accent_15};
  color: {text};
}}
.pane-window-action {{
  color: {action_window};
}}
.pane-split-action {{
  color: {action_split};
}}
.pane-close-action:hover {{
  background: {error_18};
  border-color: {error_18};
  color: {error_text};
}}
.surface-tabs {{
  margin: 4px 8px 6px;
  min-height: 24px;
  display: flex;
  align-items: center;
  gap: 6px;
}}
.surface-tab {{
  background: {border_03};
  border: 1px solid {border_07};
  border-radius: 6px;
  padding: 3px 8px;
  display: inline-flex;
  align-items: center;
  gap: 7px;
}}
.surface-tab-active {{
  background: {accent_14};
  border-color: {accent_35};
}}
.surface-tab-label {{
  color: {text_muted};
  font-size: 12px;
}}
.pane-body {{
  flex: 1;
  min-height: 0;
  position: relative;
  overflow: hidden;
}}
.surface-backdrop {{
  width: 100%;
  height: 100%;
  border-top: 1px solid {border_04};
  background:
    linear-gradient(180deg, {overlay} 0%, {elevated} 100%);
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  padding: 16px;
}}
.surface-backdrop-copy {{
  max-width: 520px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}}
.surface-backdrop-eyebrow {{
  font-weight: 600;
  font-size: 11px;
  letter-spacing: 0.10em;
  text-transform: uppercase;
  color: {text_dim};
}}
.surface-backdrop-title {{
  font-size: 18px;
  font-weight: 600;
  color: {text_bright};
}}
.surface-backdrop-note {{
  color: {text_subtle};
  font-size: 13px;
  line-height: 1.45;
}}
.surface-meta {{
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}}
.surface-chip {{
  border-radius: 999px;
  padding: 6px 10px;
  background: {border_04};
  border: 1px solid {border_06};
  color: {text_subtle};
  font-size: 12px;
}}
@media (max-width: 960px) {{
  .workspace-sidebar {{
    display: none;
  }}
  .workspace-canvas {{
    padding: 10px;
  }}
}}
"#,
        base = p.base.to_hex(),
        surface = p.surface.to_hex(),
        elevated = p.elevated.to_hex(),
        overlay = p.overlay.to_hex(),
        text = p.text.to_hex(),
        text_bright = p.text_bright.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        text_dim = p.text_dim.to_hex(),
        text_faint = p.text_faint.to_hex(),
        busy = p.busy.to_hex(),
        completed = p.completed.to_hex(),
        waiting = p.waiting.to_hex(),
        error = p.error.to_hex(),
        busy_text = p.busy_text.to_hex(),
        completed_text = p.completed_text.to_hex(),
        waiting_text = p.waiting_text.to_hex(),
        error_text = p.error_text.to_hex(),
        action_window = p.action_window.to_hex(),
        action_split = p.action_split.to_hex(),
        border_02 = rgba(p.border, 0.02),
        border_03 = rgba(p.border, 0.03),
        border_04 = rgba(p.border, 0.04),
        border_05 = rgba(p.border, 0.05),
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        border_10 = rgba(p.border, 0.10),
        accent_06 = rgba(p.accent, 0.06),
        accent_12 = rgba(p.accent, 0.12),
        accent_14 = rgba(p.accent, 0.14),
        accent_15 = rgba(p.accent, 0.15),
        accent_20 = rgba(p.accent, 0.20),
        accent_22 = rgba(p.accent, 0.22),
        accent_35 = rgba(p.accent, 0.35),
        completed_16 = rgba(p.completed, 0.16),
        waiting_18 = rgba(p.waiting, 0.18),
        error_16 = rgba(p.error, 0.16),
        error_18 = rgba(p.error, 0.18),
    );
    css
}

#[cfg(test)]
mod tests {
    use super::{default_dark, generate_css};

    #[test]
    fn generated_css_contains_legacy_shell_landmarks() {
        let css = generate_css(&default_dark());
        assert!(css.contains(".workspace-sidebar"));
        assert!(css.contains(".workspace-header"));
        assert!(css.contains(".pane-card"));
        assert!(css.contains(".surface-tabs"));
    }
}
