use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    ToggleOverview,
    CloseTerminal,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    NewWindowLeft,
    NewWindowRight,
    NewWindowUp,
    NewWindowDown,
    ResizeWindowLeft,
    ResizeWindowRight,
    ResizeWindowUp,
    ResizeWindowDown,
    ResizeSplitLeft,
    ResizeSplitRight,
    ResizeSplitUp,
    ResizeSplitDown,
    SplitRight,
    SplitDown,
}

impl ShortcutAction {
    pub const ALL: [Self; 20] = [
        Self::ToggleOverview,
        Self::CloseTerminal,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::NewWindowLeft,
        Self::NewWindowRight,
        Self::NewWindowUp,
        Self::NewWindowDown,
        Self::ResizeWindowLeft,
        Self::ResizeWindowRight,
        Self::ResizeWindowUp,
        Self::ResizeWindowDown,
        Self::ResizeSplitLeft,
        Self::ResizeSplitRight,
        Self::ResizeSplitUp,
        Self::ResizeSplitDown,
        Self::SplitRight,
        Self::SplitDown,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::ToggleOverview => "toggle_overview",
            Self::CloseTerminal => "close_terminal",
            Self::FocusLeft => "focus_left",
            Self::FocusRight => "focus_right",
            Self::FocusUp => "focus_up",
            Self::FocusDown => "focus_down",
            Self::NewWindowLeft => "new_window_left",
            Self::NewWindowRight => "new_window_right",
            Self::NewWindowUp => "new_window_up",
            Self::NewWindowDown => "new_window_down",
            Self::ResizeWindowLeft => "resize_window_left",
            Self::ResizeWindowRight => "resize_window_right",
            Self::ResizeWindowUp => "resize_window_up",
            Self::ResizeWindowDown => "resize_window_down",
            Self::ResizeSplitLeft => "resize_split_left",
            Self::ResizeSplitRight => "resize_split_right",
            Self::ResizeSplitUp => "resize_split_up",
            Self::ResizeSplitDown => "resize_split_down",
            Self::SplitRight => "split_right",
            Self::SplitDown => "split_down",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Toggle overview",
            Self::CloseTerminal => "Close terminal",
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::NewWindowLeft => "New window left",
            Self::NewWindowRight => "New window right",
            Self::NewWindowUp => "New window up",
            Self::NewWindowDown => "New window down",
            Self::ResizeWindowLeft => "Resize window left",
            Self::ResizeWindowRight => "Resize window right",
            Self::ResizeWindowUp => "Resize window up",
            Self::ResizeWindowDown => "Resize window down",
            Self::ResizeSplitLeft => "Resize split left",
            Self::ResizeSplitRight => "Resize split right",
            Self::ResizeSplitUp => "Resize split up",
            Self::ResizeSplitDown => "Resize split down",
            Self::SplitRight => "Split right",
            Self::SplitDown => "Split down",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Zoom the current workspace out to fit the full column strip.",
            Self::CloseTerminal => "Close the active pane or active top-level window.",
            Self::FocusLeft => "Move focus to the column on the left, then fall back to pane focus.",
            Self::FocusRight => {
                "Move focus to the column on the right, then fall back to pane focus."
            }
            Self::FocusUp => {
                "Move focus to the stacked window above, then fall back to pane focus."
            }
            Self::FocusDown => {
                "Move focus to the stacked window below, then fall back to pane focus."
            }
            Self::NewWindowLeft => "Create a top-level window in a new column on the left.",
            Self::NewWindowRight => "Create a top-level window in a new column on the right.",
            Self::NewWindowUp => "Create a stacked top-level window above the active window.",
            Self::NewWindowDown => "Create a stacked top-level window below the active window.",
            Self::ResizeWindowLeft => "Shrink the active column from the left edge.",
            Self::ResizeWindowRight => "Grow the active column toward the right.",
            Self::ResizeWindowUp => "Shrink the active top-level window height.",
            Self::ResizeWindowDown => "Grow the active top-level window height.",
            Self::ResizeSplitLeft => "Move the active pane split toward the left.",
            Self::ResizeSplitRight => "Move the active pane split toward the right.",
            Self::ResizeSplitUp => "Move the active pane split upward.",
            Self::ResizeSplitDown => "Move the active pane split downward.",
            Self::SplitRight => "Split the active pane to the right inside the current window.",
            Self::SplitDown => "Split the active pane downward inside the current window.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::ToggleOverview | Self::CloseTerminal => "General",
            Self::FocusLeft | Self::FocusRight | Self::FocusUp | Self::FocusDown => "Focus",
            Self::NewWindowLeft
            | Self::NewWindowRight
            | Self::NewWindowUp
            | Self::NewWindowDown => "New window",
            Self::ResizeWindowLeft
            | Self::ResizeWindowRight
            | Self::ResizeWindowUp
            | Self::ResizeWindowDown => "Resize window",
            Self::ResizeSplitLeft
            | Self::ResizeSplitRight
            | Self::ResizeSplitUp
            | Self::ResizeSplitDown => "Resize split",
            Self::SplitRight | Self::SplitDown => "Split",
        }
    }

    pub fn default_accelerators(self) -> &'static [&'static str] {
        match self {
            Self::ToggleOverview => &["<Control><Alt>o"],
            Self::CloseTerminal => &["<Control><Alt>x"],
            Self::FocusLeft => &["<Control><Alt>h", "<Control><Alt>Left"],
            Self::FocusRight => &["<Control><Alt>l", "<Control><Alt>Right"],
            Self::FocusUp => &["<Control><Alt>k", "<Control><Alt>Up"],
            Self::FocusDown => &["<Control><Alt>j", "<Control><Alt>Down"],
            Self::NewWindowLeft => &["<Control><Alt><Shift>h", "<Control><Alt><Shift>Left"],
            Self::NewWindowRight => &[
                "<Control><Alt>t",
                "<Control><Alt><Shift>l",
                "<Control><Alt><Shift>Right",
            ],
            Self::NewWindowUp => &["<Control><Alt><Shift>k", "<Control><Alt><Shift>Up"],
            Self::NewWindowDown => &[
                "<Control><Alt>g",
                "<Control><Alt><Shift>j",
                "<Control><Alt><Shift>Down",
            ],
            Self::ResizeWindowLeft => &["<Control><Alt>Home"],
            Self::ResizeWindowRight => &["<Control><Alt>End"],
            Self::ResizeWindowUp => &["<Control><Alt>Page_Up"],
            Self::ResizeWindowDown => &["<Control><Alt>Page_Down"],
            Self::ResizeSplitLeft => &["<Control><Alt><Shift>Home"],
            Self::ResizeSplitRight => &["<Control><Alt><Shift>End"],
            Self::ResizeSplitUp => &["<Control><Alt><Shift>Page_Up"],
            Self::ResizeSplitDown => &["<Control><Alt><Shift>Page_Down"],
            Self::SplitRight => &["<Control><Alt>backslash"],
            Self::SplitDown => &["<Control><Alt>minus"],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub keybindings: KeybindingConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default = "default_animations_enabled")]
    pub animations_enabled: bool,
    #[serde(default)]
    pub theme: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            keybindings: KeybindingConfig::default(),
            shell: ShellConfig::default(),
            animations_enabled: true,
            theme: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default)]
    pub program: Option<String>,
}

fn default_animations_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingConfig {
    #[serde(default)]
    pub actions: BTreeMap<String, Vec<String>>,
}

impl KeybindingConfig {
    pub fn accelerators(&self, action: ShortcutAction) -> Vec<String> {
        self.actions
            .get(action.id())
            .filter(|bindings| !bindings.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                action
                    .default_accelerators()
                    .iter()
                    .map(|binding| (*binding).to_string())
                    .collect()
            })
    }

    pub fn set_accelerators(&mut self, action: ShortcutAction, accelerators: Vec<String>) {
        self.actions.insert(action.id().into(), accelerators);
    }
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("TASKERS_CONFIG_PATH").map(PathBuf::from) {
        return path;
    }

    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .map(|path| path.join("taskers").join("config.json"))
    {
        return path;
    }

    if let Some(path) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".config").join("taskers").join("config.json"))
    {
        return path;
    }

    PathBuf::from("/tmp/taskers-config.json")
}

pub fn load_or_default(path: &Path) -> Result<AppConfig> {
    if path.exists() {
        load_config(path)
    } else {
        Ok(AppConfig::default())
    }
}

pub fn load_config(path: &Path) -> Result<AppConfig> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

pub fn save_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let data = serde_json::to_string_pretty(config)?;
    fs::write(path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::{AppConfig, ShortcutAction, load_or_default, save_config};

    #[test]
    fn roundtrips_config_files() {
        let tempdir = tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.json");
        let mut config = AppConfig::default();
        config.keybindings.set_accelerators(
            ShortcutAction::FocusRight,
            vec!["<Control><Shift>t".into()],
        );

        save_config(&config_path, &config).expect("config saved");
        let loaded = load_or_default(&config_path).expect("config loaded");

        assert_eq!(loaded, config);
    }

    #[test]
    fn fills_missing_fields_with_defaults() {
        let tempdir = tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.json");
        std::fs::write(&config_path, "{\n  \"keybindings\": {}\n}\n").expect("config written");

        let loaded = load_or_default(&config_path).expect("config loaded");

        assert_eq!(
            loaded.keybindings.accelerators(ShortcutAction::ToggleOverview),
            ShortcutAction::ToggleOverview
                .default_accelerators()
                .iter()
                .map(|binding| (*binding).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            loaded.keybindings.accelerators(ShortcutAction::FocusLeft),
            ShortcutAction::FocusLeft
                .default_accelerators()
                .iter()
                .map(|binding| (*binding).to_string())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            loaded.keybindings.accelerators(ShortcutAction::CloseTerminal),
            ShortcutAction::CloseTerminal
                .default_accelerators()
                .iter()
                .map(|binding| (*binding).to_string())
                .collect::<Vec<_>>()
        );
    }
}
