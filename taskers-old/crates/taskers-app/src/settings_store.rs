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
    OpenBrowserSplit,
    FocusBrowserAddress,
    ReloadBrowserPage,
    ToggleBrowserDevtools,
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
    pub const ALL: [Self; 24] = [
        Self::ToggleOverview,
        Self::CloseTerminal,
        Self::OpenBrowserSplit,
        Self::FocusBrowserAddress,
        Self::ReloadBrowserPage,
        Self::ToggleBrowserDevtools,
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
            Self::OpenBrowserSplit => "open_browser_split",
            Self::FocusBrowserAddress => "focus_browser_address",
            Self::ReloadBrowserPage => "reload_browser_page",
            Self::ToggleBrowserDevtools => "toggle_browser_devtools",
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
            Self::OpenBrowserSplit => "Open browser in split",
            Self::FocusBrowserAddress => "Focus browser address bar",
            Self::ReloadBrowserPage => "Reload browser page",
            Self::ToggleBrowserDevtools => "Toggle browser devtools",
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::NewWindowLeft => "New window left",
            Self::NewWindowRight => "New window right",
            Self::NewWindowUp => "New window up",
            Self::NewWindowDown => "New window down",
            Self::ResizeWindowLeft => "Make window narrower",
            Self::ResizeWindowRight => "Make window wider",
            Self::ResizeWindowUp => "Make window shorter",
            Self::ResizeWindowDown => "Make window taller",
            Self::ResizeSplitLeft => "Make split narrower",
            Self::ResizeSplitRight => "Make split wider",
            Self::ResizeSplitUp => "Make split shorter",
            Self::ResizeSplitDown => "Make split taller",
            Self::SplitRight => "Split right",
            Self::SplitDown => "Split down",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Zoom the current workspace out to fit the full column strip.",
            Self::CloseTerminal => "Close the active pane or active top-level window.",
            Self::OpenBrowserSplit => {
                "Split the active pane to the right and open a browser surface."
            }
            Self::FocusBrowserAddress => "Focus the address bar for the active browser surface.",
            Self::ReloadBrowserPage => "Reload the active browser surface.",
            Self::ToggleBrowserDevtools => "Show or hide devtools for the active browser surface.",
            Self::FocusLeft => {
                "Move focus to the column on the left, then fall back to pane focus."
            }
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
            Self::ResizeWindowLeft => "Reduce the active column width.",
            Self::ResizeWindowRight => "Increase the active column width.",
            Self::ResizeWindowUp => "Reduce the active top-level window height.",
            Self::ResizeWindowDown => "Increase the active top-level window height.",
            Self::ResizeSplitLeft => "Reduce the active split width.",
            Self::ResizeSplitRight => "Increase the active split width.",
            Self::ResizeSplitUp => "Reduce the active split height.",
            Self::ResizeSplitDown => "Increase the active split height.",
            Self::SplitRight => "Split the active pane to the right inside the current window.",
            Self::SplitDown => "Split the active pane downward inside the current window.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            Self::ToggleOverview | Self::CloseTerminal => "General",
            Self::OpenBrowserSplit
            | Self::FocusBrowserAddress
            | Self::ReloadBrowserPage
            | Self::ToggleBrowserDevtools => "Browser",
            Self::FocusLeft | Self::FocusRight | Self::FocusUp | Self::FocusDown => "Focus",
            Self::NewWindowLeft
            | Self::NewWindowRight
            | Self::NewWindowUp
            | Self::NewWindowDown => "Top-level windows",
            Self::SplitRight | Self::SplitDown => "Pane splits",
            Self::ResizeWindowLeft
            | Self::ResizeWindowRight
            | Self::ResizeWindowUp
            | Self::ResizeWindowDown
            | Self::ResizeSplitLeft
            | Self::ResizeSplitRight
            | Self::ResizeSplitUp
            | Self::ResizeSplitDown => "Advanced resize",
        }
    }

    pub fn default_accelerators(self) -> &'static [&'static str] {
        match self {
            Self::ToggleOverview => &["<Control><Alt>o"],
            Self::CloseTerminal => &["<Control><Alt>x"],
            Self::OpenBrowserSplit => &["<Control><Alt><Shift>l"],
            Self::FocusBrowserAddress => &["<Control>l"],
            Self::ReloadBrowserPage => &["<Control>r"],
            Self::ToggleBrowserDevtools => &["<Control><Shift>i"],
            Self::FocusLeft => &["<Control><Alt>h", "<Control><Alt>Left"],
            Self::FocusRight => &["<Control><Alt>l", "<Control><Alt>Right"],
            Self::FocusUp => &["<Control><Alt>k", "<Control><Alt>Up"],
            Self::FocusDown => &["<Control><Alt>j", "<Control><Alt>Down"],
            Self::NewWindowLeft => &[],
            Self::NewWindowRight => &["<Control><Alt>t"],
            Self::NewWindowUp => &[],
            Self::NewWindowDown => &["<Control><Alt>g"],
            Self::ResizeWindowLeft => &[],
            Self::ResizeWindowRight => &[],
            Self::ResizeWindowUp => &[],
            Self::ResizeWindowDown => &[],
            Self::ResizeSplitLeft => &[],
            Self::ResizeSplitRight => &[],
            Self::ResizeSplitUp => &[],
            Self::ResizeSplitDown => &[],
            Self::SplitRight => &["<Control><Alt><Shift>t"],
            Self::SplitDown => &["<Control><Alt><Shift>g"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutPreset {
    Balanced,
    PowerUser,
}

impl ShortcutPreset {
    pub const ALL: [Self; 2] = [Self::Balanced, Self::PowerUser];

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Apply Balanced Defaults",
            Self::PowerUser => "Apply Power User Defaults",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Balanced => {
                "Keep common focus, top-level window, split, overview, and close actions bound."
            }
            Self::PowerUser => {
                "Restore the dense direction and resize bindings for full keyboard-driven control."
            }
        }
    }

    pub fn accelerators(self, action: ShortcutAction) -> &'static [&'static str] {
        match self {
            Self::Balanced => action.default_accelerators(),
            Self::PowerUser => match action {
                ShortcutAction::ToggleOverview => &["<Control><Alt>o"],
                ShortcutAction::CloseTerminal => &["<Control><Alt>x"],
                ShortcutAction::OpenBrowserSplit => &["<Control><Alt><Shift>l"],
                ShortcutAction::FocusBrowserAddress => &["<Control>l"],
                ShortcutAction::ReloadBrowserPage => &["<Control>r"],
                ShortcutAction::ToggleBrowserDevtools => &["<Control><Shift>i"],
                ShortcutAction::FocusLeft => &["<Control><Alt>h", "<Control><Alt>Left"],
                ShortcutAction::FocusRight => &["<Control><Alt>l", "<Control><Alt>Right"],
                ShortcutAction::FocusUp => &["<Control><Alt>k", "<Control><Alt>Up"],
                ShortcutAction::FocusDown => &["<Control><Alt>j", "<Control><Alt>Down"],
                ShortcutAction::NewWindowLeft => {
                    &["<Control><Alt><Shift>h", "<Control><Alt><Shift>Left"]
                }
                ShortcutAction::NewWindowRight => &[
                    "<Control><Alt>t",
                    "<Control><Alt><Shift>l",
                    "<Control><Alt><Shift>Right",
                ],
                ShortcutAction::NewWindowUp => {
                    &["<Control><Alt><Shift>k", "<Control><Alt><Shift>Up"]
                }
                ShortcutAction::NewWindowDown => &[
                    "<Control><Alt>g",
                    "<Control><Alt><Shift>j",
                    "<Control><Alt><Shift>Down",
                ],
                ShortcutAction::ResizeWindowLeft => &["<Control><Alt>Home"],
                ShortcutAction::ResizeWindowRight => &["<Control><Alt>End"],
                ShortcutAction::ResizeWindowUp => &["<Control><Alt>Page_Up"],
                ShortcutAction::ResizeWindowDown => &["<Control><Alt>Page_Down"],
                ShortcutAction::ResizeSplitLeft => &["<Control><Alt><Shift>Home"],
                ShortcutAction::ResizeSplitRight => &["<Control><Alt><Shift>End"],
                ShortcutAction::ResizeSplitUp => &["<Control><Alt><Shift>Page_Up"],
                ShortcutAction::ResizeSplitDown => &["<Control><Alt><Shift>Page_Down"],
                ShortcutAction::SplitRight => &["<Control><Alt>backslash"],
                ShortcutAction::SplitDown => &["<Control><Alt>minus"],
            },
        }
    }

    pub fn keybindings(self) -> KeybindingConfig {
        if self == Self::Balanced {
            return KeybindingConfig::default();
        }

        let mut actions = BTreeMap::new();
        for action in ShortcutAction::ALL {
            let accelerators = self
                .accelerators(action)
                .iter()
                .map(|binding| (*binding).to_string())
                .collect::<Vec<_>>();
            if !accelerators.is_empty() {
                actions.insert(action.id().into(), accelerators);
            }
        }
        KeybindingConfig { actions }
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

    pub fn replace_with_preset(&mut self, preset: ShortcutPreset) {
        *self = preset.keybindings();
    }
}

pub fn default_config_path() -> PathBuf {
    taskers_paths::default_config_path()
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

    use super::{AppConfig, ShortcutAction, ShortcutPreset, load_or_default, save_config};

    #[test]
    fn roundtrips_config_files() {
        let tempdir = tempdir().expect("tempdir");
        let config_path = tempdir.path().join("config.json");
        let mut config = AppConfig::default();
        config
            .keybindings
            .set_accelerators(ShortcutAction::FocusRight, vec!["<Control><Shift>t".into()]);

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
            loaded
                .keybindings
                .accelerators(ShortcutAction::ToggleOverview),
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
            loaded
                .keybindings
                .accelerators(ShortcutAction::CloseTerminal),
            ShortcutAction::CloseTerminal
                .default_accelerators()
                .iter()
                .map(|binding| (*binding).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn balanced_defaults_leave_advanced_actions_unbound() {
        assert!(
            ShortcutAction::NewWindowLeft
                .default_accelerators()
                .is_empty()
        );
        assert!(
            ShortcutAction::NewWindowUp
                .default_accelerators()
                .is_empty()
        );
        assert!(
            ShortcutAction::ResizeWindowLeft
                .default_accelerators()
                .is_empty()
        );
        assert!(
            ShortcutAction::ResizeSplitDown
                .default_accelerators()
                .is_empty()
        );
    }

    #[test]
    fn balanced_defaults_prefer_direct_window_and_split_shortcuts() {
        assert_eq!(
            ShortcutAction::NewWindowRight.default_accelerators(),
            ["<Control><Alt>t"]
        );
        assert_eq!(
            ShortcutAction::NewWindowDown.default_accelerators(),
            ["<Control><Alt>g"]
        );
        assert_eq!(
            ShortcutAction::SplitRight.default_accelerators(),
            ["<Control><Alt><Shift>t"]
        );
        assert_eq!(
            ShortcutAction::SplitDown.default_accelerators(),
            ["<Control><Alt><Shift>g"]
        );
    }

    #[test]
    fn balanced_preset_matches_default_config_shape() {
        assert_eq!(
            ShortcutPreset::Balanced.keybindings(),
            AppConfig::default().keybindings
        );
    }

    #[test]
    fn power_user_preset_restores_dense_bindings() {
        let preset = ShortcutPreset::PowerUser.keybindings();

        assert_eq!(
            preset.accelerators(ShortcutAction::NewWindowLeft),
            vec![
                String::from("<Control><Alt><Shift>h"),
                String::from("<Control><Alt><Shift>Left")
            ]
        );
        assert_eq!(
            preset.accelerators(ShortcutAction::ResizeWindowRight),
            vec![String::from("<Control><Alt>End")]
        );
        assert_eq!(
            preset.accelerators(ShortcutAction::SplitDown),
            vec![String::from("<Control><Alt>minus")]
        );
    }
}
