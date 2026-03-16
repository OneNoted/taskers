use crate::theme::{Color, ThemePalette};

/// All built-in theme names, in display order.
pub const BUILTIN_NAMES: &[&str] = &[
    "dark",
    "catppuccin-mocha",
    "catppuccin-macchiato",
    "catppuccin-frappe",
    "catppuccin-latte",
    "tokyo-night",
    "tokyo-night-storm",
    "gruvbox-dark",
    "nord",
    "dracula",
    "solarized-dark",
    "one-dark",
    "rose-pine",
    "rose-pine-moon",
    "kanagawa",
];

/// Look up a built-in theme by config key. Returns `None` for unknown names.
pub fn builtin_theme(name: &str) -> Option<ThemePalette> {
    match name {
        "catppuccin-mocha" => Some(catppuccin_mocha()),
        "catppuccin-macchiato" => Some(catppuccin_macchiato()),
        "catppuccin-frappe" => Some(catppuccin_frappe()),
        "catppuccin-latte" => Some(catppuccin_latte()),
        "tokyo-night" => Some(tokyo_night()),
        "tokyo-night-storm" => Some(tokyo_night_storm()),
        "gruvbox-dark" => Some(gruvbox_dark()),
        "nord" => Some(nord()),
        "dracula" => Some(dracula()),
        "solarized-dark" => Some(solarized_dark()),
        "one-dark" => Some(one_dark()),
        "rose-pine" => Some(rose_pine()),
        "rose-pine-moon" => Some(rose_pine_moon()),
        "kanagawa" => Some(kanagawa()),
        _ => None,
    }
}

// ── Catppuccin ──
// https://catppuccin.com/palette/

pub fn catppuccin_mocha() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x1e, 0x1e, 0x2e),       // Base
        surface: Color::new(0x18, 0x18, 0x25),      // Mantle
        elevated: Color::new(0x31, 0x32, 0x44),      // Surface0
        overlay: Color::new(0x45, 0x47, 0x5a),       // Surface1
        text: Color::new(0xcd, 0xd6, 0xf4),          // Text
        text_bright: Color::new(0xe4, 0xe8, 0xfb),
        text_muted: Color::new(0xa6, 0xad, 0xc8),    // Subtext0
        text_subtle: Color::new(0x93, 0x99, 0xb2),   // Overlay2
        text_dim: Color::new(0x7f, 0x84, 0x9c),      // Overlay1
        text_faint: Color::new(0x6c, 0x70, 0x86),    // Overlay0
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xb4, 0xbe, 0xfe),        // Lavender
        busy: Color::new(0x89, 0xb4, 0xfa),          // Blue
        completed: Color::new(0xa6, 0xe3, 0xa1),     // Green
        waiting: Color::new(0x94, 0xe2, 0xd5),       // Teal
        error: Color::new(0xf3, 0x8b, 0xa8),         // Red
        busy_text: Color::new(0xbc, 0xd3, 0xfc),
        completed_text: Color::new(0xc3, 0xed, 0xbe),
        waiting_text: Color::new(0xb8, 0xed, 0xe6),
        error_text: Color::new(0xf7, 0xb8, 0xc8),
        action_window: Color::new(0xcb, 0xa6, 0xf7), // Mauve
        action_split: Color::new(0x89, 0xdc, 0xeb),  // Sky
        action_teal: Color::new(0x94, 0xe2, 0xd5),   // Teal
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xe4, 0xe8, 0xfb),
        agent_opencode: Color::new(0xa6, 0xad, 0xc8),
    }
}

pub fn catppuccin_macchiato() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x24, 0x27, 0x3a),
        surface: Color::new(0x1e, 0x20, 0x30),       // Mantle
        elevated: Color::new(0x36, 0x3a, 0x4f),       // Surface0
        overlay: Color::new(0x49, 0x4d, 0x64),        // Surface1
        text: Color::new(0xca, 0xd3, 0xf5),
        text_bright: Color::new(0xe1, 0xe5, 0xf9),
        text_muted: Color::new(0xa5, 0xad, 0xcb),     // Subtext0
        text_subtle: Color::new(0x93, 0x9a, 0xb7),    // Overlay2
        text_dim: Color::new(0x80, 0x87, 0xa2),       // Overlay1
        text_faint: Color::new(0x6e, 0x73, 0x8d),     // Overlay0
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xb7, 0xbd, 0xf8),         // Lavender
        busy: Color::new(0x8a, 0xad, 0xf4),           // Blue
        completed: Color::new(0xa6, 0xda, 0x95),      // Green
        waiting: Color::new(0x8b, 0xd5, 0xca),        // Teal
        error: Color::new(0xed, 0x87, 0x96),          // Red
        busy_text: Color::new(0xb8, 0xd2, 0xf8),
        completed_text: Color::new(0xc3, 0xea, 0xbc),
        waiting_text: Color::new(0xb3, 0xe6, 0xdf),
        error_text: Color::new(0xf4, 0xb3, 0xbc),
        action_window: Color::new(0xc6, 0xa0, 0xf6),  // Mauve
        action_split: Color::new(0x91, 0xd7, 0xe3),   // Sky
        action_teal: Color::new(0x8b, 0xd5, 0xca),    // Teal
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xe1, 0xe5, 0xf9),
        agent_opencode: Color::new(0xa5, 0xad, 0xcb),
    }
}

pub fn catppuccin_frappe() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x30, 0x34, 0x46),
        surface: Color::new(0x29, 0x2c, 0x3c),        // Mantle
        elevated: Color::new(0x41, 0x45, 0x59),        // Surface0
        overlay: Color::new(0x51, 0x57, 0x6d),         // Surface1
        text: Color::new(0xc6, 0xd0, 0xf5),
        text_bright: Color::new(0xde, 0xe2, 0xf7),
        text_muted: Color::new(0xa5, 0xad, 0xce),      // Subtext0
        text_subtle: Color::new(0x94, 0x9c, 0xbb),     // Overlay2
        text_dim: Color::new(0x83, 0x8b, 0xa7),        // Overlay1
        text_faint: Color::new(0x73, 0x79, 0x94),      // Overlay0
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xba, 0xbb, 0xf1),          // Lavender
        busy: Color::new(0x8c, 0xaa, 0xee),            // Blue
        completed: Color::new(0xa6, 0xd1, 0x89),       // Green
        waiting: Color::new(0x81, 0xc8, 0xbe),         // Teal
        error: Color::new(0xe7, 0x82, 0x84),           // Red
        busy_text: Color::new(0xb8, 0xce, 0xf5),
        completed_text: Color::new(0xc4, 0xe5, 0xb2),
        waiting_text: Color::new(0xaf, 0xe0, 0xd8),
        error_text: Color::new(0xf0, 0xb2, 0xb3),
        action_window: Color::new(0xca, 0x9e, 0xe6),   // Mauve
        action_split: Color::new(0x99, 0xd1, 0xdb),    // Sky
        action_teal: Color::new(0x81, 0xc8, 0xbe),     // Teal
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xde, 0xe2, 0xf7),
        agent_opencode: Color::new(0xa5, 0xad, 0xce),
    }
}

pub fn catppuccin_latte() -> ThemePalette {
    ThemePalette {
        base: Color::new(0xef, 0xf1, 0xf5),           // Base
        surface: Color::new(0xe6, 0xe9, 0xef),         // Mantle
        elevated: Color::new(0xcc, 0xd0, 0xda),        // Surface0
        overlay: Color::new(0xbc, 0xc0, 0xcc),         // Surface1
        text: Color::new(0x4c, 0x4f, 0x69),            // Text
        text_bright: Color::new(0x2a, 0x2c, 0x3e),
        text_muted: Color::new(0x6c, 0x6f, 0x85),      // Subtext0
        text_subtle: Color::new(0x7c, 0x7f, 0x93),     // Overlay2
        text_dim: Color::new(0x8c, 0x8f, 0xa1),        // Overlay1
        text_faint: Color::new(0x9c, 0xa0, 0xb0),      // Overlay0
        border: Color::new(0x00, 0x00, 0x00),           // Black for light theme
        accent: Color::new(0x72, 0x87, 0xfd),           // Lavender
        busy: Color::new(0x1e, 0x66, 0xf5),            // Blue
        completed: Color::new(0x40, 0xa0, 0x2b),       // Green
        waiting: Color::new(0x17, 0x92, 0x99),          // Teal
        error: Color::new(0xd2, 0x0f, 0x39),           // Red
        busy_text: Color::new(0x1e, 0x66, 0xf5),
        completed_text: Color::new(0x40, 0xa0, 0x2b),
        waiting_text: Color::new(0x17, 0x92, 0x99),
        error_text: Color::new(0xd2, 0x0f, 0x39),
        action_window: Color::new(0x88, 0x39, 0xef),   // Mauve
        action_split: Color::new(0x04, 0xa5, 0xe5),    // Sky
        action_teal: Color::new(0x17, 0x92, 0x99),     // Teal
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0x2a, 0x2c, 0x3e),
        agent_opencode: Color::new(0x6c, 0x6f, 0x85),
    }
}

// ── Tokyo Night ──
// https://github.com/folke/tokyonight.nvim

pub fn tokyo_night() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x1a, 0x1b, 0x26),
        surface: Color::new(0x16, 0x16, 0x1e),        // bg_dark
        elevated: Color::new(0x29, 0x2e, 0x42),       // bg_highlight
        overlay: Color::new(0x41, 0x48, 0x68),         // terminal_black
        text: Color::new(0xc0, 0xca, 0xf5),            // fg
        text_bright: Color::new(0xdc, 0xe0, 0xf8),
        text_muted: Color::new(0xa9, 0xb1, 0xd6),      // fg_dark
        text_subtle: Color::new(0x73, 0x7a, 0xa2),     // dark5
        text_dim: Color::new(0x56, 0x5f, 0x89),        // comment
        text_faint: Color::new(0x3b, 0x42, 0x61),      // fg_gutter
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x7d, 0xcf, 0xff),           // cyan
        busy: Color::new(0x7a, 0xa2, 0xf7),            // blue
        completed: Color::new(0x9e, 0xce, 0x6a),       // green
        waiting: Color::new(0x7d, 0xcf, 0xff),          // cyan
        error: Color::new(0xf7, 0x76, 0x8e),           // red
        busy_text: Color::new(0xb0, 0xc8, 0xfa),
        completed_text: Color::new(0xc4, 0xe4, 0xa6),
        waiting_text: Color::new(0xb0, 0xe3, 0xff),
        error_text: Color::new(0xfa, 0xb0, 0xbc),
        action_window: Color::new(0xbb, 0x9a, 0xf7),   // purple
        action_split: Color::new(0x2a, 0xc3, 0xde),    // blue1
        action_teal: Color::new(0x1a, 0xbc, 0x9c),     // teal
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xdc, 0xe0, 0xf8),
        agent_opencode: Color::new(0xa9, 0xb1, 0xd6),
    }
}

pub fn tokyo_night_storm() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x24, 0x28, 0x3b),
        surface: Color::new(0x1f, 0x23, 0x35),
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
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xdc, 0xe0, 0xf8),
        agent_opencode: Color::new(0xa9, 0xb1, 0xd6),
    }
}

// ── Gruvbox ──
// https://github.com/morhetz/gruvbox

pub fn gruvbox_dark() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x28, 0x28, 0x28),           // bg
        surface: Color::new(0x1d, 0x20, 0x21),        // bg0_h
        elevated: Color::new(0x3c, 0x38, 0x36),       // bg1
        overlay: Color::new(0x50, 0x49, 0x45),         // bg2
        text: Color::new(0xeb, 0xdb, 0xb2),            // fg
        text_bright: Color::new(0xfb, 0xf1, 0xc7),    // fg0
        text_muted: Color::new(0xd5, 0xc4, 0xa1),      // fg2
        text_subtle: Color::new(0xbd, 0xae, 0x93),     // fg3
        text_dim: Color::new(0xa8, 0x99, 0x84),        // fg4
        text_faint: Color::new(0x92, 0x83, 0x74),      // gray
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x83, 0xa5, 0x98),           // bright aqua
        busy: Color::new(0x83, 0xa5, 0x98),            // bright blue
        completed: Color::new(0xb8, 0xbb, 0x26),       // bright green
        waiting: Color::new(0x8e, 0xc0, 0x7c),          // bright aqua
        error: Color::new(0xfb, 0x49, 0x34),           // bright red
        busy_text: Color::new(0xb4, 0xcf, 0xc5),
        completed_text: Color::new(0xd5, 0xd7, 0x8a),
        waiting_text: Color::new(0xbc, 0xdb, 0xac),
        error_text: Color::new(0xfc, 0xa0, 0x9a),
        action_window: Color::new(0xd3, 0x86, 0x9b),   // bright purple
        action_split: Color::new(0x8e, 0xc0, 0x7c),    // bright aqua
        action_teal: Color::new(0x68, 0x9d, 0x6a),     // aqua
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xfb, 0xf1, 0xc7),
        agent_opencode: Color::new(0xd5, 0xc4, 0xa1),
    }
}

// ── Nord ──
// https://www.nordtheme.com/docs/colors-and-palettes

pub fn nord() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x2e, 0x34, 0x40),           // nord0
        surface: Color::new(0x27, 0x2c, 0x36),
        elevated: Color::new(0x3b, 0x42, 0x52),        // nord1
        overlay: Color::new(0x43, 0x4c, 0x5e),         // nord2
        text: Color::new(0xd8, 0xde, 0xe9),             // nord4
        text_bright: Color::new(0xec, 0xef, 0xf4),     // nord6
        text_muted: Color::new(0xb8, 0xc0, 0xcc),
        text_subtle: Color::new(0x8e, 0x96, 0xa3),
        text_dim: Color::new(0x6c, 0x76, 0x89),
        text_faint: Color::new(0x4c, 0x56, 0x6a),      // nord3
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x88, 0xc0, 0xd0),           // nord8 Frost
        busy: Color::new(0x81, 0xa1, 0xc1),            // nord9
        completed: Color::new(0xa3, 0xbe, 0x8c),       // nord14 green
        waiting: Color::new(0x8f, 0xbc, 0xbb),          // nord7 frost
        error: Color::new(0xbf, 0x61, 0x6a),           // nord11 red
        busy_text: Color::new(0xb3, 0xc8, 0xdb),
        completed_text: Color::new(0xc8, 0xdb, 0xb4),
        waiting_text: Color::new(0xb8, 0xd8, 0xd6),
        error_text: Color::new(0xdb, 0xa1, 0xa7),
        action_window: Color::new(0xb4, 0x8e, 0xad),   // nord15 purple
        action_split: Color::new(0x88, 0xc0, 0xd0),    // nord8
        action_teal: Color::new(0x8f, 0xbc, 0xbb),     // nord7
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xec, 0xef, 0xf4),
        agent_opencode: Color::new(0xb8, 0xc0, 0xcc),
    }
}

// ── Dracula ──
// https://draculatheme.com/spec

pub fn dracula() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x28, 0x2a, 0x36),            // Background
        surface: Color::new(0x21, 0x22, 0x2c),
        elevated: Color::new(0x34, 0x37, 0x46),
        overlay: Color::new(0x44, 0x47, 0x5a),          // Current Line
        text: Color::new(0xf8, 0xf8, 0xf2),             // Foreground
        text_bright: Color::new(0xff, 0xff, 0xff),
        text_muted: Color::new(0xc5, 0xc8, 0xd6),
        text_subtle: Color::new(0xa4, 0xa8, 0xb8),
        text_dim: Color::new(0x62, 0x72, 0xa4),         // Comment
        text_faint: Color::new(0x4e, 0x52, 0x66),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xbd, 0x93, 0xf9),           // Purple
        busy: Color::new(0x8b, 0xe9, 0xfd),             // Cyan
        completed: Color::new(0x50, 0xfa, 0x7b),        // Green
        waiting: Color::new(0xbd, 0x93, 0xf9),           // Purple
        error: Color::new(0xff, 0x55, 0x55),             // Red
        busy_text: Color::new(0xbd, 0xf0, 0xfe),
        completed_text: Color::new(0x9e, 0xfc, 0xb4),
        waiting_text: Color::new(0xdb, 0xc8, 0xfc),
        error_text: Color::new(0xff, 0x99, 0x99),
        action_window: Color::new(0xff, 0x79, 0xc6),    // Pink
        action_split: Color::new(0x8b, 0xe9, 0xfd),     // Cyan
        action_teal: Color::new(0x50, 0xfa, 0x7b),      // Green
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xff, 0xff, 0xff),
        agent_opencode: Color::new(0xc5, 0xc8, 0xd6),
    }
}

// ── Solarized ──
// https://ethanschoonover.com/solarized/

pub fn solarized_dark() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x00, 0x2b, 0x36),            // base03
        surface: Color::new(0x00, 0x1e, 0x27),
        elevated: Color::new(0x07, 0x36, 0x42),         // base02
        overlay: Color::new(0x0a, 0x40, 0x50),
        text: Color::new(0x83, 0x94, 0x96),              // base0
        text_bright: Color::new(0x93, 0xa1, 0xa1),      // base1
        text_muted: Color::new(0x6d, 0x82, 0x86),
        text_subtle: Color::new(0x58, 0x6e, 0x75),      // base01
        text_dim: Color::new(0x46, 0x56, 0x5c),
        text_faint: Color::new(0x2e, 0x42, 0x48),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x26, 0x8b, 0xd2),            // blue
        busy: Color::new(0x26, 0x8b, 0xd2),             // blue
        completed: Color::new(0x85, 0x99, 0x00),        // green
        waiting: Color::new(0x2a, 0xa1, 0x98),           // cyan
        error: Color::new(0xdc, 0x32, 0x2f),             // red
        busy_text: Color::new(0x6c, 0xb0, 0xde),
        completed_text: Color::new(0xb0, 0xc4, 0x4d),
        waiting_text: Color::new(0x6d, 0xc5, 0xbd),
        error_text: Color::new(0xe8, 0x75, 0x6f),
        action_window: Color::new(0x6c, 0x71, 0xc4),    // violet
        action_split: Color::new(0x2a, 0xa1, 0x98),     // cyan
        action_teal: Color::new(0x2a, 0xa1, 0x98),      // cyan
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0x93, 0xa1, 0xa1),
        agent_opencode: Color::new(0x6d, 0x82, 0x86),
    }
}

// ── One Dark ──
// https://github.com/Binaryify/OneDark-Pro

pub fn one_dark() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x28, 0x2c, 0x34),
        surface: Color::new(0x21, 0x25, 0x2b),
        elevated: Color::new(0x2c, 0x31, 0x3a),
        overlay: Color::new(0x35, 0x3b, 0x45),
        text: Color::new(0xab, 0xb2, 0xbf),
        text_bright: Color::new(0xd7, 0xda, 0xe0),
        text_muted: Color::new(0x9d, 0xa5, 0xb4),
        text_subtle: Color::new(0x84, 0x8b, 0x98),
        text_dim: Color::new(0x5c, 0x63, 0x70),
        text_faint: Color::new(0x49, 0x51, 0x62),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x61, 0xaf, 0xef),            // blue
        busy: Color::new(0x61, 0xaf, 0xef),              // blue
        completed: Color::new(0x98, 0xc3, 0x79),         // green
        waiting: Color::new(0x56, 0xb6, 0xc2),            // cyan
        error: Color::new(0xe0, 0x6c, 0x75),              // red
        busy_text: Color::new(0x9d, 0xcd, 0xf5),
        completed_text: Color::new(0xc1, 0xdc, 0xa8),
        waiting_text: Color::new(0x96, 0xd4, 0xda),
        error_text: Color::new(0xeb, 0xa8, 0xad),
        action_window: Color::new(0xc6, 0x78, 0xdd),     // purple
        action_split: Color::new(0x56, 0xb6, 0xc2),      // cyan
        action_teal: Color::new(0x56, 0xb6, 0xc2),       // cyan
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xd7, 0xda, 0xe0),
        agent_opencode: Color::new(0x9d, 0xa5, 0xb4),
    }
}

// ── Rosé Pine ──
// https://rosepinetheme.com/palette/

pub fn rose_pine() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x19, 0x17, 0x24),             // Base
        surface: Color::new(0x1f, 0x1d, 0x2e),          // Surface
        elevated: Color::new(0x26, 0x23, 0x3a),         // Overlay
        overlay: Color::new(0x40, 0x3d, 0x52),           // Highlight Med
        text: Color::new(0xe0, 0xde, 0xf4),              // Text
        text_bright: Color::new(0xee, 0xed, 0xff),
        text_muted: Color::new(0x90, 0x8c, 0xaa),        // Subtle
        text_subtle: Color::new(0x81, 0x7d, 0x9c),
        text_dim: Color::new(0x6e, 0x6a, 0x86),          // Muted
        text_faint: Color::new(0x52, 0x4f, 0x67),        // Highlight High
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xc4, 0xa7, 0xe7),             // Iris
        busy: Color::new(0xc4, 0xa7, 0xe7),              // Iris
        completed: Color::new(0x9c, 0xcf, 0xd8),         // Foam
        waiting: Color::new(0xf6, 0xc1, 0x77),            // Gold
        error: Color::new(0xeb, 0x6f, 0x92),              // Love
        busy_text: Color::new(0xdd, 0xd0, 0xf2),
        completed_text: Color::new(0xc5, 0xe5, 0xe9),
        waiting_text: Color::new(0xf9, 0xdd, 0xb4),
        error_text: Color::new(0xf4, 0xa5, 0xb8),
        action_window: Color::new(0xeb, 0xbc, 0xba),     // Rose
        action_split: Color::new(0x9c, 0xcf, 0xd8),      // Foam
        action_teal: Color::new(0x31, 0x74, 0x8f),        // Pine
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xee, 0xed, 0xff),
        agent_opencode: Color::new(0x90, 0x8c, 0xaa),
    }
}

pub fn rose_pine_moon() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x23, 0x21, 0x36),
        surface: Color::new(0x2a, 0x27, 0x3f),
        elevated: Color::new(0x39, 0x35, 0x52),
        overlay: Color::new(0x44, 0x41, 0x5a),
        text: Color::new(0xe0, 0xde, 0xf4),
        text_bright: Color::new(0xee, 0xed, 0xff),
        text_muted: Color::new(0x90, 0x8c, 0xaa),
        text_subtle: Color::new(0x81, 0x7d, 0x9c),
        text_dim: Color::new(0x6e, 0x6a, 0x86),
        text_faint: Color::new(0x56, 0x52, 0x6e),
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0xc4, 0xa7, 0xe7),
        busy: Color::new(0xc4, 0xa7, 0xe7),              // Iris
        completed: Color::new(0x9c, 0xcf, 0xd8),         // Foam
        waiting: Color::new(0xf6, 0xc1, 0x77),            // Gold
        error: Color::new(0xeb, 0x6f, 0x92),              // Love
        busy_text: Color::new(0xdd, 0xd0, 0xf2),
        completed_text: Color::new(0xc5, 0xe5, 0xe9),
        waiting_text: Color::new(0xf9, 0xdd, 0xb4),
        error_text: Color::new(0xf4, 0xa5, 0xb8),
        action_window: Color::new(0xea, 0x9a, 0x97),     // Rose
        action_split: Color::new(0x9c, 0xcf, 0xd8),      // Foam
        action_teal: Color::new(0x3e, 0x8f, 0xb0),        // Pine
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xee, 0xed, 0xff),
        agent_opencode: Color::new(0x90, 0x8c, 0xaa),
    }
}

// ── Kanagawa ──
// https://github.com/rebelot/kanagawa.nvim

pub fn kanagawa() -> ThemePalette {
    ThemePalette {
        base: Color::new(0x1f, 0x1f, 0x28),            // sumiInk3
        surface: Color::new(0x16, 0x16, 0x1d),          // sumiInk0
        elevated: Color::new(0x2a, 0x2a, 0x37),         // sumiInk4
        overlay: Color::new(0x36, 0x36, 0x46),           // sumiInk5
        text: Color::new(0xdc, 0xd7, 0xba),              // fujiWhite
        text_bright: Color::new(0xec, 0xea, 0xd5),
        text_muted: Color::new(0xc8, 0xc0, 0x93),        // oldWhite
        text_subtle: Color::new(0xa0, 0x9e, 0x7d),
        text_dim: Color::new(0x72, 0x71, 0x69),
        text_faint: Color::new(0x54, 0x54, 0x6d),        // sumiInk6
        border: Color::new(0xff, 0xff, 0xff),
        accent: Color::new(0x7e, 0x9c, 0xd8),             // crystalBlue
        busy: Color::new(0x7e, 0x9c, 0xd8),              // crystalBlue
        completed: Color::new(0x98, 0xbb, 0x6c),         // springGreen
        waiting: Color::new(0x7a, 0xa8, 0x9f),            // waveAqua2
        error: Color::new(0xe4, 0x68, 0x76),              // waveRed
        busy_text: Color::new(0xb3, 0xc5, 0xe8),
        completed_text: Color::new(0xc2, 0xd7, 0xa4),
        waiting_text: Color::new(0xaf, 0xd0, 0xc6),
        error_text: Color::new(0xf0, 0xa3, 0xab),
        action_window: Color::new(0x95, 0x7f, 0xb8),     // oniViolet
        action_split: Color::new(0x7a, 0xa8, 0x9f),      // waveAqua2
        action_teal: Color::new(0x6a, 0x95, 0x89),        // waveAqua1
        agent_claude: Color::new(0xd9, 0x77, 0x57),
        agent_codex: Color::new(0xec, 0xea, 0xd5),
        agent_opencode: Color::new(0xc8, 0xc0, 0x93),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::generate_css;

    #[test]
    fn all_builtin_themes_generate_css() {
        for name in BUILTIN_NAMES {
            if *name == "dark" {
                // default_dark() is tested separately in theme::tests
                continue;
            }
            let palette = builtin_theme(name)
                .unwrap_or_else(|| panic!("builtin_theme({name:?}) returned None"));
            let css = generate_css(&palette);
            assert!(
                css.contains(".workspace-sidebar"),
                "theme {name} missing .workspace-sidebar"
            );
            assert!(
                css.contains(".pane-card"),
                "theme {name} missing .pane-card"
            );
            assert!(
                css.contains(".status-dot-busy"),
                "theme {name} missing .status-dot-busy"
            );
        }
    }

    #[test]
    fn builtin_lookup_returns_none_for_unknown() {
        assert!(builtin_theme("nonexistent").is_none());
    }
}
