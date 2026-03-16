use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    ToggleOverview,
    NewTerminal,
    CloseTerminal,
}

impl ShortcutAction {
    pub const ALL: [Self; 3] = [Self::ToggleOverview, Self::NewTerminal, Self::CloseTerminal];

    pub fn label(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Toggle overview",
            Self::NewTerminal => "New terminal",
            Self::CloseTerminal => "Close terminal",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::ToggleOverview => "Zoom the current workspace out to fit every visible pane.",
            Self::NewTerminal => "Create a new top-level terminal window in the current workspace.",
            Self::CloseTerminal => "Close the active pane or active terminal window.",
        }
    }

    pub fn default_accelerator(self) -> &'static str {
        match self {
            Self::ToggleOverview => "<Control><Alt>o",
            Self::NewTerminal => "<Control><Alt>t",
            Self::CloseTerminal => "<Control><Alt>x",
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeybindingConfig {
    #[serde(default = "default_toggle_overview")]
    pub toggle_overview: String,
    #[serde(default = "default_new_terminal")]
    pub new_terminal: String,
    #[serde(default = "default_close_terminal")]
    pub close_terminal: String,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            toggle_overview: default_toggle_overview(),
            new_terminal: default_new_terminal(),
            close_terminal: default_close_terminal(),
        }
    }
}

impl KeybindingConfig {
    pub fn accelerator(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::ToggleOverview => &self.toggle_overview,
            ShortcutAction::NewTerminal => &self.new_terminal,
            ShortcutAction::CloseTerminal => &self.close_terminal,
        }
    }

    pub fn set_accelerator(&mut self, action: ShortcutAction, accelerator: String) {
        match action {
            ShortcutAction::ToggleOverview => self.toggle_overview = accelerator,
            ShortcutAction::NewTerminal => self.new_terminal = accelerator,
            ShortcutAction::CloseTerminal => self.close_terminal = accelerator,
        }
    }
}

fn default_toggle_overview() -> String {
    ShortcutAction::ToggleOverview
        .default_accelerator()
        .to_string()
}

fn default_new_terminal() -> String {
    ShortcutAction::NewTerminal
        .default_accelerator()
        .to_string()
}

fn default_close_terminal() -> String {
    ShortcutAction::CloseTerminal
        .default_accelerator()
        .to_string()
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
        config
            .keybindings
            .set_accelerator(ShortcutAction::NewTerminal, "<Control><Shift>t".into());

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
                .accelerator(ShortcutAction::ToggleOverview),
            ShortcutAction::ToggleOverview.default_accelerator()
        );
        assert_eq!(
            loaded.keybindings.accelerator(ShortcutAction::NewTerminal),
            ShortcutAction::NewTerminal.default_accelerator()
        );
        assert_eq!(
            loaded
                .keybindings
                .accelerator(ShortcutAction::CloseTerminal),
            ShortcutAction::CloseTerminal.default_accelerator()
        );
    }
}
