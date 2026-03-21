use std::{
    cell::RefCell,
    fmt::{self, Write as _},
    path::PathBuf,
    str::FromStr,
};

use serde::{Deserialize, Serialize, de};

// ── Color primitive ──

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

fn rgba(c: Color, a: f32) -> String {
    format!("rgba({},{},{},{:.2})", c.r, c.g, c.b, a)
}

impl FromStr for Color {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim().trim_start_matches('#');
        if s.len() != 6 {
            return Err(format!("expected 6-char hex color, got {s:?}"));
        }
        let r = u8::from_str_radix(&s[0..2], 16).map_err(|e| e.to_string())?;
        let g = u8::from_str_radix(&s[2..4], 16).map_err(|e| e.to_string())?;
        let b = u8::from_str_radix(&s[4..6], 16).map_err(|e| e.to_string())?;
        Ok(Self { r, g, b })
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

// ── Agent icon color (cairo) ──

#[derive(Clone, Copy)]
pub struct AgentIconColor {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl From<Color> for AgentIconColor {
    fn from(c: Color) -> Self {
        Self {
            red: f64::from(c.r) / 255.0,
            green: f64::from(c.g) / 255.0,
            blue: f64::from(c.b) / 255.0,
            alpha: 1.0,
        }
    }
}

// ── Theme palette ──

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePalette {
    // Surfaces
    pub base: Color,
    pub surface: Color,
    pub elevated: Color,
    pub overlay: Color,
    // Text hierarchy
    pub text: Color,
    pub text_bright: Color,
    pub text_muted: Color,
    pub text_subtle: Color,
    pub text_dim: Color,
    pub text_faint: Color,
    // Border base (white for dark themes, black for light)
    pub border: Color,
    // Accent
    pub accent: Color,
    // Status
    pub busy: Color,
    pub completed: Color,
    pub waiting: Color,
    pub error: Color,
    // Status tint text
    pub busy_text: Color,
    pub completed_text: Color,
    pub waiting_text: Color,
    pub error_text: Color,
    // Action accents
    pub action_window: Color,
    pub action_split: Color,
    pub action_teal: Color,
    // Agent brand colors
    pub agent_claude: Color,
    pub agent_codex: Color,
    pub agent_opencode: Color,
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
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xf0, 0xf2, 0xf8),
        agent_opencode: Color::new(0xc4, 0xca, 0xd4),
    }
}

// ── Partial palette (for TOML partial override) ──

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PartialPalette {
    pub base: Option<Color>,
    pub surface: Option<Color>,
    pub elevated: Option<Color>,
    pub overlay: Option<Color>,
    pub text: Option<Color>,
    pub text_bright: Option<Color>,
    pub text_muted: Option<Color>,
    pub text_subtle: Option<Color>,
    pub text_dim: Option<Color>,
    pub text_faint: Option<Color>,
    pub border: Option<Color>,
    pub accent: Option<Color>,
    pub busy: Option<Color>,
    pub completed: Option<Color>,
    pub waiting: Option<Color>,
    pub error: Option<Color>,
    pub busy_text: Option<Color>,
    pub completed_text: Option<Color>,
    pub waiting_text: Option<Color>,
    pub error_text: Option<Color>,
    pub action_window: Option<Color>,
    pub action_split: Option<Color>,
    pub action_teal: Option<Color>,
    pub agent_claude: Option<Color>,
    pub agent_codex: Option<Color>,
    pub agent_opencode: Option<Color>,
}

impl PartialPalette {
    pub fn resolve(self, default: &ThemePalette) -> ThemePalette {
        ThemePalette {
            base: self.base.unwrap_or(default.base),
            surface: self.surface.unwrap_or(default.surface),
            elevated: self.elevated.unwrap_or(default.elevated),
            overlay: self.overlay.unwrap_or(default.overlay),
            text: self.text.unwrap_or(default.text),
            text_bright: self.text_bright.unwrap_or(default.text_bright),
            text_muted: self.text_muted.unwrap_or(default.text_muted),
            text_subtle: self.text_subtle.unwrap_or(default.text_subtle),
            text_dim: self.text_dim.unwrap_or(default.text_dim),
            text_faint: self.text_faint.unwrap_or(default.text_faint),
            border: self.border.unwrap_or(default.border),
            accent: self.accent.unwrap_or(default.accent),
            busy: self.busy.unwrap_or(default.busy),
            completed: self.completed.unwrap_or(default.completed),
            waiting: self.waiting.unwrap_or(default.waiting),
            error: self.error.unwrap_or(default.error),
            busy_text: self.busy_text.unwrap_or(default.busy_text),
            completed_text: self.completed_text.unwrap_or(default.completed_text),
            waiting_text: self.waiting_text.unwrap_or(default.waiting_text),
            error_text: self.error_text.unwrap_or(default.error_text),
            action_window: self.action_window.unwrap_or(default.action_window),
            action_split: self.action_split.unwrap_or(default.action_split),
            action_teal: self.action_teal.unwrap_or(default.action_teal),
            agent_claude: self.agent_claude.unwrap_or(default.agent_claude),
            agent_codex: self.agent_codex.unwrap_or(default.agent_codex),
            agent_opencode: self.agent_opencode.unwrap_or(default.agent_opencode),
        }
    }
}

// ── Theme definition (TOML document) ──

#[derive(Debug, Clone, Deserialize)]
pub struct ThemeDefinition {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub palette: PartialPalette,
}

// ── Theme loading ──

fn theme_dir() -> Option<PathBuf> {
    Some(taskers_paths::default_theme_dir())
}

pub fn load_theme(
    theme_name: Option<&str>,
    builtin_lookup: impl Fn(&str) -> Option<ThemePalette>,
) -> (String, ThemePalette) {
    let default = default_dark();

    let Some(name) = theme_name else {
        return ("dark".into(), default);
    };

    if name == "dark" {
        return ("dark".into(), default);
    }

    // Check built-in themes first.
    if let Some(palette) = builtin_lookup(name) {
        return (name.into(), palette);
    }

    // Fall back to TOML file on disk.
    let Some(dir) = theme_dir() else {
        eprintln!("warning: cannot determine theme directory");
        return ("dark".into(), default);
    };

    let path = dir.join(format!("{name}.toml"));
    match std::fs::read_to_string(&path) {
        Ok(content) => match toml::from_str::<ThemeDefinition>(&content) {
            Ok(def) => {
                let resolved_name = def.name.unwrap_or_else(|| name.into());
                let palette = def.palette.resolve(&default);
                (resolved_name, palette)
            }
            Err(e) => {
                eprintln!("warning: failed to parse theme {name}: {e}");
                ("dark".into(), default)
            }
        },
        Err(_) => {
            eprintln!("warning: theme file not found: {}", path.display());
            ("dark".into(), default)
        }
    }
}

// ── Thread-local state for live theme switching ──

thread_local! {
    static ACTIVE_PALETTE: RefCell<Option<ThemePalette>> = const { RefCell::new(None) };
    static CSS_PROVIDER: RefCell<Option<gtk::CssProvider>> = const { RefCell::new(None) };
}

/// First-time theme installation: creates the CssProvider, attaches it to
/// the display, and stores both it and the palette for later live updates.
pub fn install_theme(palette: ThemePalette) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(&generate_css(&palette));

    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    CSS_PROVIDER.with(|cell| {
        *cell.borrow_mut() = Some(provider);
    });
    ACTIVE_PALETTE.with(|cell| {
        *cell.borrow_mut() = Some(palette);
    });
}

/// Live-switch to a new theme without restarting. Reloads CSS on the
/// existing provider and updates the palette thread-local.
pub fn apply_theme(palette: ThemePalette) {
    let css = generate_css(&palette);
    CSS_PROVIDER.with(|cell| {
        if let Some(provider) = cell.borrow().as_ref() {
            provider.load_from_data(&css);
        }
    });
    ACTIVE_PALETTE.with(|cell| {
        *cell.borrow_mut() = Some(palette);
    });
}

pub fn resolve_agent_icon_color(agent_kind: &str) -> Option<AgentIconColor> {
    ACTIVE_PALETTE.with(|cell| {
        let palette = cell.borrow();
        let palette = palette.as_ref()?;
        let color = match agent_kind {
            "claude" => palette.agent_claude,
            "codex" => palette.agent_codex,
            "opencode" => palette.agent_opencode,
            _ => return None,
        };
        Some(AgentIconColor::from(color))
    })
}

// ── CSS generation ──

pub fn generate_css(p: &ThemePalette) -> String {
    let mut css = String::with_capacity(8192);

    // Using a closure to keep writes concise — unwrap is safe on String writes.
    let w = &mut css;

    // ── Base ──
    let _ = write!(
        w,
        "
        window {{
            background: {base};
            color: {text};
            font-family: \"IBM Plex Sans\", \"SF Pro Text\", system-ui, sans-serif;
        }}

        label {{
            line-height: 1.35;
        }}

        ",
        base = p.base.to_hex(),
        text = p.text.to_hex(),
    );

    // ── Paned separators ──
    let _ = write!(
        w,
        "
        paned > separator {{
            background: {border_04};
            min-width: 1px;
            min-height: 1px;
            padding: 0;
        }}
        ",
        border_04 = rgba(p.border, 0.04),
    );

    // ── Sidebar ──
    let _ = write!(
        w,
        "
        .workspace-sidebar {{
            background: {surface};
            border-right: 1px solid {border_04};
            padding: 4px 6px;
        }}

        .sidebar-heading {{
            font-weight: 600;
            font-size: 0.74rem;
            color: {text_dim};
            letter-spacing: 0.10em;
            text-transform: uppercase;
            margin-bottom: 2px;
        }}

        .workspace-add {{
            background: transparent;
            color: {text_dim};
            border: 1px solid {border_10};
            border-radius: 999px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.95rem;
            transition: background 180ms ease-in-out, color 180ms ease-in-out, border-color 180ms ease-in-out;
        }}

        .workspace-add:hover {{
            background: {waiting_10};
            color: {text};
            border-color: {waiting_24};
        }}

        .workspace-add:active {{
            background: {waiting_16};
        }}

        .workspace-button {{
            padding: 0;
        }}

        .workspace-button:hover .workspace-item {{
            background: {border_04};
            border-color: {border_10};
        }}

        .workspace-item {{
            padding: 7px 8px;
            border-radius: 8px;
            border: 1px solid transparent;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .workspace-item-active {{
            background: {border_05};
            border-color: {border_10};
        }}

        .workspace-label {{
            font-weight: 600;
            color: {text_bright};
            font-size: 0.80rem;
        }}

        .workspace-agent-icon,
        .activity-agent-icon,
        .pane-agent-icon,
        .surface-tab-agent-icon {{
            opacity: 0.96;
        }}

        .agent-icon-codex {{
            color: {agent_codex};
        }}

        .agent-icon-claude {{
            color: {agent_claude};
        }}

        .agent-icon-opencode {{
            color: {agent_opencode};
        }}

        .workspace-preview {{
            color: {text_subtle};
            font-size: 0.72rem;
        }}

        .workspace-meta {{
            color: {text_dim};
            font-size: 0.70rem;
            letter-spacing: 0.01em;
        }}

        .workspace-status-badge {{
            background: {accent_14};
            color: {busy_text};
            border-radius: 999px;
            padding: 0 5px;
            min-width: 18px;
            min-height: 18px;
            font-size: 0.62rem;
            font-weight: 700;
            letter-spacing: 0.04em;
        }}

        .workspace-status-badge-dot {{
            background: transparent;
            min-width: 14px;
            min-height: 14px;
            padding: 0;
            font-size: 0.46rem;
        }}

        .workspace-status-badge-idle {{
            color: {text_faint};
            opacity: 0;
        }}

        .workspace-status-badge-state-busy {{
            background: {busy_16};
            color: {busy_text};
        }}

        .workspace-status-badge-state-completed {{
            background: {completed_16};
            color: {completed_text};
        }}

        .workspace-status-badge-state-waiting {{
            background: {waiting_18};
            color: {waiting_text};
        }}

        .workspace-status-badge-state-error {{
            background: {error_16};
            color: {error_text};
        }}

        .workspace-item-has-attention {{
            border-color: {border_10};
        }}

        .workspace-item-state-busy {{
            background: {busy_05};
            border-color: {busy_16};
        }}

        .workspace-item-state-completed {{
            background: {completed_06};
            border-color: {completed_16};
        }}

        .workspace-item-state-waiting {{
            background: {waiting_08};
            border-color: {waiting_20};
        }}

        .workspace-item-state-error {{
            background: {error_08};
            border-color: {error_18};
        }}

        .workspace-item-has-unread .workspace-label {{
            color: {text_bright};
        }}

        .workspace-item-active.workspace-item-state-busy {{
            background: {busy_10};
            border-color: {busy_24};
        }}

        .workspace-item-active.workspace-item-state-completed {{
            background: {completed_09};
            border-color: {completed_22};
        }}

        .workspace-item-active.workspace-item-state-waiting {{
            background: {waiting_12};
            border-color: {waiting_30};
        }}

        .workspace-item-active.workspace-item-state-error {{
            background: {error_10};
            border-color: {error_24};
        }}

        .workspace-close {{
            background: transparent;
            color: {text_faint};
            border-radius: 4px;
            min-width: 22px;
            min-height: 22px;
            padding: 0;
            font-size: 0.85rem;
            opacity: 0;
            transition: opacity 160ms ease-in-out, background 160ms ease-in-out, color 160ms ease-in-out;
        }}

        .workspace-row:hover .workspace-close {{
            opacity: 1;
        }}

        .workspace-close-visible {{
            opacity: 1;
        }}

        .workspace-close:hover {{
            background: {error_15};
            color: {error};
        }}

        .workspace-rename-entry {{
            background: {accent_08};
            color: {text};
            border: 1px solid {accent_30};
            border-radius: 4px;
            padding: 4px 6px;
            font-size: 0.82rem;
            font-weight: 500;
            min-height: 0;
        }}

        .workspace-rename-entry:focus {{
            border-color: {accent_55};
        }}
        ",
        surface = p.surface.to_hex(),
        border_04 = rgba(p.border, 0.04),
        border_05 = rgba(p.border, 0.05),
        border_10 = rgba(p.border, 0.10),
        text = p.text.to_hex(),
        text_bright = p.text_bright.to_hex(),
        text_dim = p.text_dim.to_hex(),
        text_faint = p.text_faint.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        agent_codex = p.agent_codex.to_hex(),
        agent_claude = p.agent_claude.to_hex(),
        agent_opencode = p.agent_opencode.to_hex(),
        accent_08 = rgba(p.accent, 0.08),
        accent_14 = rgba(p.accent, 0.14),
        accent_30 = rgba(p.accent, 0.30),
        accent_55 = rgba(p.accent, 0.55),
        busy_text = p.busy_text.to_hex(),
        completed_text = p.completed_text.to_hex(),
        waiting_text = p.waiting_text.to_hex(),
        error_text = p.error_text.to_hex(),
        busy_05 = rgba(p.busy, 0.05),
        busy_10 = rgba(p.busy, 0.10),
        busy_16 = rgba(p.busy, 0.16),
        busy_24 = rgba(p.busy, 0.24),
        completed_06 = rgba(p.completed, 0.06),
        completed_09 = rgba(p.completed, 0.09),
        completed_16 = rgba(p.completed, 0.16),
        completed_22 = rgba(p.completed, 0.22),
        waiting_08 = rgba(p.waiting, 0.08),
        waiting_10 = rgba(p.waiting, 0.10),
        waiting_12 = rgba(p.waiting, 0.12),
        waiting_16 = rgba(p.waiting, 0.16),
        waiting_18 = rgba(p.waiting, 0.18),
        waiting_20 = rgba(p.waiting, 0.20),
        waiting_24 = rgba(p.waiting, 0.24),
        waiting_30 = rgba(p.waiting, 0.30),
        error = p.error.to_hex(),
        error_08 = rgba(p.error, 0.08),
        error_10 = rgba(p.error, 0.10),
        error_15 = rgba(p.error, 0.15),
        error_16 = rgba(p.error, 0.16),
        error_18 = rgba(p.error, 0.18),
        error_24 = rgba(p.error, 0.24),
    );

    // ── Workspace header ──
    let _ = write!(
        w,
        "
        .workspace-header {{
            border-bottom: 1px solid {border_07};
        }}

        .workspace-header-label {{
            font-weight: 600;
            font-size: 0.84rem;
            color: {text_bright};
        }}

        .workspace-header-action {{
            background: transparent;
            color: {text_faint};
            border-radius: 4px;
            min-width: 24px;
            min-height: 24px;
            padding: 0;
            font-size: 0.85rem;
            transition: background 180ms ease-in-out, color 180ms ease-in-out;
        }}

        .workspace-header-action:hover {{
            background: {border_06};
            color: {text_muted};
        }}

        .workspace-header-action-active {{
            background: {accent_14};
            color: {text_bright};
        }}

        .workspace-header-title-btn {{
            background: transparent;
            padding: 0 6px;
            min-height: 24px;
        }}

        .workspace-header-title-btn:hover {{
            background: {border_06};
        }}

        .workspace-header-close {{
            color: {text_faint};
        }}

        .workspace-header-close:hover {{
            background: {error_15};
            color: {error};
        }}
        ",
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        accent_14 = rgba(p.accent, 0.14),
        error = p.error.to_hex(),
        error_15 = rgba(p.error, 0.15),
        text_bright = p.text_bright.to_hex(),
        text_faint = p.text_faint.to_hex(),
        text_muted = p.text_muted.to_hex(),
    );

    // ── Attention panel ──
    let _ = write!(
        w,
        "
        .attention-panel {{
            background: {surface};
            border-left: 1px solid {border_04};
            padding: 6px 12px;
        }}

        .activity-item-button {{
            padding: 0;
        }}

        .activity-item {{
            background: transparent;
            border-left: 2px solid transparent;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .activity-item-button:hover .activity-item {{
            background: {border_03};
        }}

        .activity-item-state-busy {{
            border-left-color: {busy_55};
        }}

        .activity-item-state-completed {{
            border-left-color: {completed_55};
        }}

        .activity-item-state-waiting {{
            border-left-color: {waiting_70};
        }}

        .activity-item-state-error {{
            border-left-color: {error_65};
        }}

        .activity-meta {{
            color: {text_dim};
            font-size: 0.70rem;
        }}

        .activity-preview {{
            color: {text_subtle};
            font-size: 0.74rem;
        }}

        .activity-action {{
            background: transparent;
            color: {text_dim};
            border: 1px solid {border_10};
            border-radius: 999px;
            padding: 2px 8px;
            font-size: 0.70rem;
            font-weight: 600;
            min-height: 0;
            transition: background 160ms ease-in-out, color 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .activity-action:hover {{
            background: {waiting_10};
            color: {waiting_text};
            border-color: {waiting_25};
        }}

        .activity-time {{
            color: {text_faint};
            font-size: 0.70rem;
        }}
        ",
        surface = p.surface.to_hex(),
        border_03 = rgba(p.border, 0.03),
        border_04 = rgba(p.border, 0.04),
        border_10 = rgba(p.border, 0.10),
        text_dim = p.text_dim.to_hex(),
        text_faint = p.text_faint.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        waiting_text = p.waiting_text.to_hex(),
        busy_55 = rgba(p.busy, 0.55),
        completed_55 = rgba(p.completed, 0.55),
        waiting_10 = rgba(p.waiting, 0.10),
        waiting_25 = rgba(p.waiting, 0.25),
        waiting_70 = rgba(p.waiting, 0.70),
        error_65 = rgba(p.error, 0.65),
    );

    // ── Workspace windows ──
    let _ = write!(
        w,
        "
        .workspace-window {{
            background: {elevated};
            border: 1px solid {border_07};
            border-radius: 6px;
        }}

        .workspace-window-active {{
            border-color: {border_14};
        }}

        .workspace-window-state-busy {{
            border-color: {busy_22};
        }}

        .workspace-window-state-completed {{
            border-color: {completed_22};
        }}

        .workspace-window-state-waiting {{
            border-color: {waiting_30};
        }}

        .workspace-window-state-error {{
            border-color: {error_24};
        }}

        .workspace-window-active.workspace-window-state-busy {{
            border-color: {busy_38};
        }}

        .workspace-window-active.workspace-window-state-completed {{
            border-color: {completed_34};
        }}

        .workspace-window-active.workspace-window-state-waiting {{
            border-color: {waiting_48};
        }}

        .workspace-window-active.workspace-window-state-error {{
            border-color: {error_38};
        }}

        .workspace-window-toolbar {{
            background: {border_03};
            border-bottom: 1px solid {border_06};
            border-top-left-radius: 6px;
            border-top-right-radius: 6px;
            padding: 3px 0;
        }}

        .workspace-window-toolbar-title {{
            font-size: 0.82rem;
            font-weight: 600;
            color: {text_bright};
        }}

        .workspace-window-toolbar-actions {{
            background: {border_04};
            border: 1px solid {border_06};
            border-radius: 999px;
            padding: 2px;
        }}

        .workspace-window-toolbar-action {{
            background: transparent;
            color: {text_faint};
            border: 1px solid transparent;
            border-radius: 999px;
            min-width: 24px;
            min-height: 22px;
            padding: 0 8px;
            font-size: 0.75rem;
            font-weight: 600;
            transition: background 160ms ease-in-out, color 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .workspace-window-toolbar-action:hover {{
            background: {border_08};
            border-color: {border_10};
            color: {text_bright};
        }}

        .workspace-window-ghost {{
            background: {elevated_78};
            border-style: dashed;
            box-shadow: 0 12px 28px rgba(0,0,0,0.26);
        }}

        .workspace-window-ghost-chrome {{
            padding: 6px;
            spacing: 0;
        }}

        .workspace-window-ghost-header {{
            margin: 0;
            padding: 2px 6px;
        }}

        .workspace-window-ghost-strip {{
            margin: 4px 0 6px;
        }}

        .workspace-window-ghost-tab {{
            background: {border_05};
            border-color: {border_10};
        }}

        .workspace-window-ghost-body {{
            margin: 0 2px 2px;
            border-radius: 4px;
            background: {border_03};
            border: 1px solid {border_04};
        }}

        .workspace-window-resize-handle {{
            background: {border_03};
            transition: background 160ms ease-in-out;
        }}

        .workspace-window-resize-handle-right:hover,
        .workspace-window-resize-handle-bottom:hover {{
            background: {accent_14};
        }}

        .workspace-window-resize-handle-active {{
            background: {accent_24};
        }}
        ",
        elevated = p.elevated.to_hex(),
        elevated_78 = rgba(p.elevated, 0.78),
        border_03 = rgba(p.border, 0.03),
        border_04 = rgba(p.border, 0.04),
        border_05 = rgba(p.border, 0.05),
        border_06 = rgba(p.border, 0.06),
        border_08 = rgba(p.border, 0.08),
        border_07 = rgba(p.border, 0.07),
        border_10 = rgba(p.border, 0.10),
        border_14 = rgba(p.border, 0.14),
        accent_14 = rgba(p.accent, 0.14),
        accent_24 = rgba(p.accent, 0.24),
        text_bright = p.text_bright.to_hex(),
        text_faint = p.text_faint.to_hex(),
        busy_22 = rgba(p.busy, 0.22),
        busy_38 = rgba(p.busy, 0.38),
        completed_22 = rgba(p.completed, 0.22),
        completed_34 = rgba(p.completed, 0.34),
        waiting_30 = rgba(p.waiting, 0.30),
        waiting_48 = rgba(p.waiting, 0.48),
        error_24 = rgba(p.error, 0.24),
        error_38 = rgba(p.error, 0.38),
    );

    // ── Pane cards ──
    let _ = write!(
        w,
        "
        .pane-card {{
            background: transparent;
        }}

        .pane-header {{
            background: {border_02};
            border-bottom: 1px solid {border_05};
            padding: 4px 2px;
            transition: background 160ms ease-in-out;
        }}

        .pane-header:hover {{
            background: {border_04};
        }}

        .pane-card-active .pane-header {{
            background: {accent_06};
            border-bottom: 1px solid {accent_15};
        }}

        .pane-card-active .pane-header:hover {{
            background: {accent_10};
        }}

        .pane-card-state-busy .pane-header {{
            background: {busy_04};
            border-bottom-color: {busy_16};
        }}

        .pane-card-state-completed .pane-header {{
            background: {completed_04};
            border-bottom-color: {completed_16};
        }}

        .pane-card-state-waiting .pane-header {{
            background: {waiting_06};
            border-bottom-color: {waiting_18};
        }}

        .pane-card-state-error .pane-header {{
            background: {error_05};
            border-bottom-color: {error_16};
        }}

        .pane-card-active.pane-card-state-waiting .pane-header {{
            background: {waiting_10};
            border-bottom-color: {waiting_24};
        }}

        .pane-title {{
            font-weight: 500;
            color: {text_muted};
            font-size: 0.74rem;
        }}

        .pane-card-active .pane-title {{
            color: {text};
        }}

        .pane-close {{
            background: transparent;
            color: {text_faint};
            border-radius: 3px;
            min-width: 18px;
            min-height: 18px;
            padding: 0;
            font-size: 0.75rem;
            transition: background 160ms ease-in-out, color 160ms ease-in-out;
        }}

        .pane-close:hover {{
            background: {error_15};
            color: {error};
        }}

        .pane-action-cluster {{
            background: {border_04};
            border: 1px solid {border_05};
            border-radius: 999px;
            padding: 2px;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .pane-header:hover .pane-action-cluster {{
            background: {border_05};
            border-color: {border_06};
        }}

        .pane-card-active .pane-action-cluster {{
            background: {accent_06};
            border-color: {accent_12};
        }}

        .pane-card-active .pane-header:hover .pane-action-cluster {{
            background: {accent_10};
            border-color: {accent_15};
        }}

        .pane-card-active .pane-close-action {{
            background: {error_10};
            border-color: {error_16};
            color: {error_soft};
        }}

        .pane-card-active .pane-close-action:hover {{
            background: {error_18};
            border-color: {error_18};
            color: {error_text};
        }}

        .pane-action {{
            background: transparent;
            color: {text_faint};
            border: 1px solid transparent;
            border-radius: 999px;
            min-width: 20px;
            min-height: 20px;
            padding: 0 6px;
            font-size: 0.72rem;
            font-weight: 600;
            opacity: 0;
            transition: opacity 180ms ease-in-out, background 160ms ease-in-out, color 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .pane-header:hover .pane-action {{
            opacity: 0.76;
        }}

        .pane-card-active .pane-action {{
            opacity: 0.86;
        }}

        .pane-card-active .pane-window-action {{
            background: {waiting_10};
            border-color: {waiting_18};
            color: {action_window};
        }}

        .pane-card-active .pane-split-action {{
            background: {teal_11};
            border-color: {teal_18};
            color: {action_split};
        }}

        .pane-action:hover {{
            opacity: 1;
            background: {accent_12};
            border-color: {accent_15};
            color: {text};
        }}

        .pane-card-active .pane-window-action:hover {{
            background: {waiting_20};
            border-color: {waiting_24};
            color: {action_window_hover};
        }}

        .pane-card-active .pane-split-action:hover {{
            background: {teal_18};
            border-color: {teal_18};
            color: {action_split_hover};
        }}

        .pane-meta {{
            color: {text_faint};
            font-size: 0.75rem;
        }}
        ",
        border_02 = rgba(p.border, 0.02),
        border_04 = rgba(p.border, 0.04),
        border_05 = rgba(p.border, 0.05),
        border_06 = rgba(p.border, 0.06),
        text = p.text.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_faint = p.text_faint.to_hex(),
        error = p.error.to_hex(),
        error_text = p.error_text.to_hex(),
        // #f9a8a8 – midpoint between error and error_text
        error_soft = Color::new(0xf9, 0xa8, 0xa8).to_hex(),
        accent_06 = rgba(p.accent, 0.06),
        accent_10 = rgba(p.accent, 0.10),
        accent_12 = rgba(p.accent, 0.12),
        accent_15 = rgba(p.accent, 0.15),
        busy_04 = rgba(p.busy, 0.04),
        busy_16 = rgba(p.busy, 0.16),
        completed_04 = rgba(p.completed, 0.04),
        completed_16 = rgba(p.completed, 0.16),
        waiting_06 = rgba(p.waiting, 0.06),
        waiting_10 = rgba(p.waiting, 0.10),
        waiting_18 = rgba(p.waiting, 0.18),
        waiting_20 = rgba(p.waiting, 0.20),
        waiting_24 = rgba(p.waiting, 0.24),
        error_05 = rgba(p.error, 0.05),
        error_10 = rgba(p.error, 0.10),
        error_15 = rgba(p.error, 0.15),
        error_16 = rgba(p.error, 0.16),
        error_18 = rgba(p.error, 0.18),
        action_window = p.action_window.to_hex(),
        action_split = p.action_split.to_hex(),
        // Hover variants: lighten towards white
        action_window_hover = Color::new(0xd8, 0xf4, 0xff).to_hex(),
        action_split_hover = Color::new(0xcc, 0xfb, 0xf1).to_hex(),
        teal_11 = rgba(p.action_teal, 0.11),
        teal_18 = rgba(p.action_teal, 0.18),
    );

    // ── Surface tabs ──
    let _ = write!(
        w,
        "
        .surface-tabs {{
            margin: 4px 8px 6px;
            min-height: 24px;
        }}

        .surface-tab {{
            background: {border_03};
            border: 1px solid {border_07};
            border-radius: 6px;
            padding: 3px 8px;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .surface-tab-active {{
            background: {accent_14};
            border-color: {accent_35};
        }}

        .surface-tab-has-attention.surface-tab-state-busy {{
            background: {busy_08};
            border-color: {busy_22};
        }}

        .surface-tab-has-attention.surface-tab-state-completed {{
            background: {completed_08};
            border-color: {completed_22};
        }}

        .surface-tab-has-attention.surface-tab-state-waiting {{
            background: {waiting_10};
            border-color: {waiting_28};
        }}

        .surface-tab-has-attention.surface-tab-state-error {{
            background: {error_10};
            border-color: {error_28};
        }}

        .surface-tab-dragging {{
            background: {border_08};
            border-color: {border_18};
        }}

        .surface-tab-exiting {{
            border-color: {border_04};
        }}

        .surface-tab-label,
        .surface-tab-close,
        .surface-tab-add {{
            min-height: 0;
            padding: 0;
        }}

        .surface-tab-label {{
            color: {text_muted};
            font-size: 0.74rem;
        }}

        .surface-tab-active .surface-tab-label {{
            color: {text_bright};
        }}

        .surface-tab-title {{
            color: {text_muted};
            font-size: 0.74rem;
        }}

        .surface-tab-active .surface-tab-title {{
            color: {text_bright};
        }}

        .surface-tab-close,
        .surface-tab-add {{
            color: {text_dim};
            border-radius: 4px;
            min-width: 18px;
            min-height: 18px;
        }}

        .surface-tab-close:hover {{
            background: {border_06};
            color: {text_subtle};
        }}

        .surface-tab-add {{
            color: {text_bright};
            background: {accent_14};
            border: 1px solid {accent_35};
            min-width: 22px;
            min-height: 20px;
            font-size: 0.82rem;
            font-weight: 700;
        }}

        .surface-tab-add:hover {{
            background: {accent_22};
            border-color: {accent_48};
            color: {text_bright};
        }}
        ",
        border_03 = rgba(p.border, 0.03),
        border_04 = rgba(p.border, 0.04),
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        border_08 = rgba(p.border, 0.08),
        border_18 = rgba(p.border, 0.18),
        text_bright = p.text_bright.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_dim = p.text_dim.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        accent_14 = rgba(p.accent, 0.14),
        accent_22 = rgba(p.accent, 0.22),
        accent_35 = rgba(p.accent, 0.35),
        accent_48 = rgba(p.accent, 0.48),
        busy_08 = rgba(p.busy, 0.08),
        busy_22 = rgba(p.busy, 0.22),
        completed_08 = rgba(p.completed, 0.08),
        completed_22 = rgba(p.completed, 0.22),
        waiting_10 = rgba(p.waiting, 0.10),
        waiting_28 = rgba(p.waiting, 0.28),
        error_10 = rgba(p.error, 0.10),
        error_28 = rgba(p.error, 0.28),
    );

    // ── Inline header tabs ──
    let _ = write!(
        w,
        "
        .pane-header-tabs {{
            margin: 0 2px;
        }}

        .inline-tab {{
            background: {inline_bg};
            border: 1px solid {inline_border};
            border-radius: 4px;
            padding: 0 4px;
            min-height: 20px;
            font-size: 0.74rem;
            color: {text_dim};
            transition: background 120ms ease-in-out;
        }}

        .inline-tab:hover {{
            background: {inline_hover};
        }}

        .inline-tab-active {{
            background: {inline_active_bg};
            border-color: {inline_active_border};
            color: {text_bright};
        }}

        .inline-tab-button {{
            padding: 0;
            min-height: 0;
            background: transparent;
            color: inherit;
        }}

        .inline-tab-button:hover {{
            background: transparent;
        }}

        .inline-tab-label {{
            color: {text_dim};
            font-size: 0.74rem;
        }}

        .inline-tab-active .inline-tab-label {{
            color: {text_bright};
        }}

        .inline-tab-close {{
            min-width: 14px;
            min-height: 14px;
            padding: 0;
            margin: 0;
            font-size: 0.65rem;
            color: {text_faint};
            background: transparent;
            border-radius: 3px;
        }}

        .inline-tab-close:hover {{
            background: {inline_close_hover};
            color: {text_subtle};
        }}

        .inline-tab-add {{
            min-width: 22px;
            min-height: 20px;
            padding: 0;
            font-size: 0.82rem;
            font-weight: 700;
            color: {text_bright};
            background: {inline_add_bg};
            border: 1px solid {inline_add_border};
            border-radius: 4px;
        }}

        .inline-tab-add:hover {{
            background: {inline_add_hover};
            border-color: {inline_add_hover_border};
            color: {text_bright};
        }}
        ",
        inline_bg = rgba(p.border, 0.03),
        inline_border = rgba(p.border, 0.07),
        inline_hover = rgba(p.border, 0.06),
        inline_active_bg = rgba(p.accent, 0.14),
        inline_active_border = rgba(p.accent, 0.35),
        inline_add_bg = rgba(p.accent, 0.14),
        inline_add_border = rgba(p.accent, 0.35),
        inline_add_hover = rgba(p.accent, 0.22),
        inline_add_hover_border = rgba(p.accent, 0.48),
        inline_close_hover = rgba(p.border, 0.06),
        text_bright = p.text_bright.to_hex(),
        text_dim = p.text_dim.to_hex(),
        text_faint = p.text_faint.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
    );

    // ── Status dots ──
    let _ = write!(
        w,
        "
        .status-dot {{
            font-size: 0.55rem;
            min-width: 10px;
            min-height: 10px;
        }}

        .status-dot-normal {{ color: {text_faint}; }}
        .status-dot-busy {{ color: {busy}; }}
        .status-dot-completed {{ color: {completed}; }}
        .status-dot-waiting {{ color: {waiting}; }}
        .status-dot-error {{ color: {error}; }}
        ",
        text_faint = p.text_faint.to_hex(),
        busy = p.busy.to_hex(),
        completed = p.completed.to_hex(),
        waiting = p.waiting.to_hex(),
        error = p.error.to_hex(),
    );

    // ── Empty state ──
    let _ = write!(
        w,
        "
        .empty-state {{
            color: {text_faint};
            font-size: 0.80rem;
            margin-top: 4px;
        }}
        ",
        text_faint = p.text_faint.to_hex(),
    );

    // ── Terminal ──
    let _ = write!(
        w,
        "
        .terminal-output,
        .terminal-entry {{
            border-radius: 0;
            background: {base};
            color: {text};
            font-family: Monospace;
        }}

        .terminal-output {{
            padding: 4px;
        }}

        .terminal-entry {{
            padding: 6px 8px;
            border-top: 1px solid {border_07};
        }}

        .terminal-entry:focus {{
            border-top: 1px solid {accent_40};
        }}
        ",
        base = p.base.to_hex(),
        text = p.text.to_hex(),
        border_07 = rgba(p.border, 0.07),
        accent_40 = rgba(p.accent, 0.40),
    );

    // ── Popover / context menus ──
    let _ = write!(
        w,
        "
        popover > contents {{
            background: {overlay};
            border: 1px solid {border_10};
            border-radius: 8px;
            padding: 4px;
            box-shadow: 0 8px 24px rgba(0,0,0,0.4);
        }}

        .context-item {{
            color: {text_subtle};
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 160ms ease-in-out;
        }}

        .context-item:hover {{
            background: {accent_12};
            color: {text};
        }}

        .context-separator {{
            background: {border_07};
            margin: 4px 8px;
            min-height: 1px;
        }}

        popover .destructive-action {{
            color: {error};
            font-size: 0.8rem;
            padding: 6px 12px;
            border-radius: 4px;
            min-height: 0;
            transition: background 160ms ease-in-out;
        }}

        popover .destructive-action:hover {{
            background: {error_12};
        }}
        ",
        overlay = p.overlay.to_hex(),
        border_07 = rgba(p.border, 0.07),
        border_10 = rgba(p.border, 0.10),
        text = p.text.to_hex(),
        text_subtle = p.text_subtle.to_hex(),
        error = p.error.to_hex(),
        accent_12 = rgba(p.accent, 0.12),
        error_12 = rgba(p.error, 0.12),
    );

    // ── Utility classes ──
    let _ = write!(
        w,
        "
        .dim-label {{
            opacity: 0.6;
        }}

        .monospace {{
            font-family: Monospace;
        }}
        ",
    );

    // ── Settings dialog ──
    let _ = write!(
        w,
        "
        .settings-section-title {{
            font-weight: 700;
            font-size: 0.76rem;
            color: {text_dim};
            letter-spacing: 0.08em;
            text-transform: uppercase;
        }}

        .settings-row {{
            padding: 6px 0;
        }}

        .theme-card {{
            background: {border_03};
            border: 1px solid {border_07};
            border-radius: 8px;
            padding: 8px;
            transition: background 160ms ease-in-out, border-color 160ms ease-in-out;
        }}

        .theme-card:hover {{
            background: {border_06};
            border-color: {border_12};
        }}

        .theme-card-active {{
            border-color: {accent_40};
            background: {accent_06};
        }}

        .theme-card-active:hover {{
            background: {accent_10};
        }}

        .theme-card-label {{
            font-size: 0.70rem;
            font-weight: 600;
            color: {text_muted};
        }}

        .theme-card-active .theme-card-label {{
            color: {text_bright};
        }}
        ",
        text_dim = p.text_dim.to_hex(),
        text_muted = p.text_muted.to_hex(),
        text_bright = p.text_bright.to_hex(),
        border_03 = rgba(p.border, 0.03),
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        border_12 = rgba(p.border, 0.12),
        accent_06 = rgba(p.accent, 0.06),
        accent_10 = rgba(p.accent, 0.10),
        accent_40 = rgba(p.accent, 0.40),
    );

    // ── Settings navigation ──
    let _ = write!(
        w,
        "
        .settings-nav {{
            margin: 10px 18px 2px 18px;
            padding: 2px;
            background: {border_03};
            border-radius: 7px;
            border: 1px solid {border_07};
        }}

        .settings-nav button {{
            font-size: 0.72rem;
            font-weight: 600;
            letter-spacing: 0.03em;
            padding: 5px 16px;
            border-radius: 5px;
            min-height: 0;
            min-width: 0;
            background: transparent;
            color: {text_dim};
            border: 1px solid transparent;
            transition: background 120ms ease-in-out,
                        color 120ms ease-in-out,
                        border-color 120ms ease-in-out;
            box-shadow: none;
            outline: none;
        }}

        .settings-nav button:hover {{
            background: {border_06};
            color: {text_muted};
        }}

        .settings-nav button:checked {{
            background: {accent_10};
            color: {accent};
            border-color: {accent_20};
            box-shadow: 0 1px 3px rgba(0,0,0,0.18);
        }}

        .settings-nav button:checked:hover {{
            background: {accent_14};
        }}

        .settings-keybind-category {{
            font-weight: 700;
            font-size: 0.70rem;
            color: {accent_60};
            letter-spacing: 0.07em;
            text-transform: uppercase;
            margin-top: 16px;
            margin-bottom: 4px;
            padding-bottom: 5px;
            border-bottom: 1px solid {border_10};
        }}

        .settings-theme-family {{
            font-weight: 700;
            font-size: 0.70rem;
            color: {accent_60};
            letter-spacing: 0.07em;
            text-transform: uppercase;
            margin-top: 12px;
            margin-bottom: 4px;
        }}

        .settings-search {{
            font-size: 0.78rem;
            padding: 4px 8px;
            border-radius: 6px;
            background: {border_03};
            border: 1px solid {border_07};
        }}

        .settings-keybind-btn {{
            padding: 2px 10px;
            min-height: 0;
            min-width: 0;
            border-radius: 5px;
            border: 1px solid {border_07};
            background: {border_03};
            transition: background 120ms ease-in-out, border-color 120ms ease-in-out;
        }}

        .settings-keybind-btn:hover {{
            background: {border_06};
            border-color: {border_12};
        }}

        .settings-keybind-value {{
            font-size: 0.72rem;
        }}

        .settings-reset-btn {{
            padding: 2px 4px;
            min-height: 0;
            min-width: 0;
            font-size: 0.72rem;
            opacity: 0.4;
            transition: opacity 120ms ease-in-out;
        }}

        .settings-reset-btn:hover {{
            opacity: 1.0;
        }}
        ",
        border_03 = rgba(p.border, 0.03),
        border_06 = rgba(p.border, 0.06),
        border_07 = rgba(p.border, 0.07),
        border_10 = rgba(p.border, 0.10),
        border_12 = rgba(p.border, 0.12),
        text_dim = p.text_dim.to_hex(),
        text_muted = p.text_muted.to_hex(),
        accent = p.accent.to_hex(),
        accent_10 = rgba(p.accent, 0.10),
        accent_14 = rgba(p.accent, 0.14),
        accent_20 = rgba(p.accent, 0.20),
        accent_60 = rgba(p.accent, 0.60),
    );

    css
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_hex_roundtrip() {
        let c = Color::new(0x0f, 0x11, 0x17);
        assert_eq!(c.to_hex(), "#0f1117");
        assert_eq!("#0f1117".parse::<Color>().unwrap(), c);
    }

    #[test]
    fn color_rgba_format() {
        let c = Color::new(124, 138, 255);
        assert_eq!(rgba(c, 0.14), "rgba(124,138,255,0.14)");
    }

    #[test]
    fn partial_palette_resolves_defaults() {
        let default = default_dark();
        let partial = PartialPalette {
            base: Some(Color::new(0x1e, 0x1e, 0x2e)),
            ..Default::default()
        };
        let resolved = partial.resolve(&default);
        assert_eq!(resolved.base, Color::new(0x1e, 0x1e, 0x2e));
        assert_eq!(resolved.surface, default.surface);
        assert_eq!(resolved.accent, default.accent);
    }

    #[test]
    fn default_dark_generates_css() {
        let css = generate_css(&default_dark());
        assert!(css.contains("#0f1117"));
        assert!(css.contains("#e2e4ea"));
        assert!(css.contains("rgba(124,138,255,"));
        assert!(css.contains(".workspace-sidebar"));
        assert!(css.contains(".pane-card"));
        assert!(css.contains(".status-dot-busy"));
    }

    #[test]
    fn toml_theme_parses() {
        let toml_str = r##"
            name = "Test Theme"

            [palette]
            base = "#1e1e2e"
            accent = "#cba6f7"
        "##;
        let def: ThemeDefinition = toml::from_str(toml_str).unwrap();
        assert_eq!(def.name.as_deref(), Some("Test Theme"));
        assert_eq!(def.palette.base, Some(Color::new(0x1e, 0x1e, 0x2e)));
        assert_eq!(def.palette.accent, Some(Color::new(0xcb, 0xa6, 0xf7)));
        assert!(def.palette.surface.is_none());
    }

    #[test]
    fn agent_icon_color_from_palette() {
        let c = Color::new(0xd9, 0x77, 0x57);
        let icon: AgentIconColor = c.into();
        assert!((icon.red - 0.851).abs() < 0.001);
        assert!((icon.green - 0.467).abs() < 0.001);
        assert!((icon.blue - 0.341).abs() < 0.001);
        assert!((icon.alpha - 1.0).abs() < f64::EPSILON);
    }
}
