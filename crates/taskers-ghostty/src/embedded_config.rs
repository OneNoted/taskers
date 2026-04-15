use std::{
    fs, io,
    path::{Path, PathBuf},
};

use thiserror::Error;

use crate::backend::EmbeddedTerminalAppearance;

const MANAGED_HEADER: &str = concat!(
    "# Taskers managed embedded-terminal Ghostty config.\n",
    "#\n",
    "# The Taskers GUI owns the settings in the managed block below and may\n",
    "# rewrite them. Advanced manual settings belong in override.conf.\n",
);
const MANAGED_BEGIN: &str = "# BEGIN TASKERS MANAGED SETTINGS";
const MANAGED_END: &str = "# END TASKERS MANAGED SETTINGS";

const KEY_THEME: &str = "theme";
const KEY_FONT_FAMILY: &str = "font-family";
const KEY_FONT_SIZE: &str = "font-size";
const KEY_WINDOW_PADDING_X: &str = "window-padding-x";
const KEY_WINDOW_PADDING_Y: &str = "window-padding-y";
const KEY_CURSOR_STYLE: &str = "cursor-style";
const KEY_CURSOR_STYLE_BLINK: &str = "cursor-style-blink";
const KEY_SCROLLBACK_LIMIT: &str = "scrollback-limit";
const KEY_BACKGROUND_OPACITY: &str = "background-opacity";
const KEY_BACKGROUND_OPACITY_CELLS: &str = "background-opacity-cells";

const MANAGED_KEYS: &[&str] = &[
    KEY_THEME,
    KEY_FONT_FAMILY,
    KEY_FONT_SIZE,
    KEY_WINDOW_PADDING_X,
    KEY_WINDOW_PADDING_Y,
    KEY_CURSOR_STYLE,
    KEY_CURSOR_STYLE_BLINK,
    KEY_SCROLLBACK_LIMIT,
    KEY_BACKGROUND_OPACITY,
    KEY_BACKGROUND_OPACITY_CELLS,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OptionalBoolValue {
    #[default]
    Default,
    Enabled,
    Disabled,
}

impl OptionalBoolValue {
    fn from_config_value(value: &str) -> Self {
        match value.trim() {
            "true" => Self::Enabled,
            "false" => Self::Disabled,
            _ => Self::Default,
        }
    }

    fn config_value(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Enabled => Some("true"),
            Self::Disabled => Some("false"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EmbeddedTerminalConfig {
    pub theme: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<String>,
    pub window_padding_x: Option<String>,
    pub window_padding_y: Option<String>,
    pub cursor_style: Option<String>,
    pub cursor_style_blink: OptionalBoolValue,
    pub scrollback_limit: Option<String>,
    pub background_opacity: Option<String>,
    pub background_opacity_cells: OptionalBoolValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedTerminalConfigPaths {
    pub dir: PathBuf,
    pub base: PathBuf,
    pub override_file: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddedTerminalConfigState {
    pub paths: EmbeddedTerminalConfigPaths,
    pub managed: EmbeddedTerminalConfig,
    pub startup_notes: Vec<String>,
}

#[derive(Debug, Error)]
pub enum EmbeddedTerminalConfigError {
    #[error("failed to create embedded terminal config dir at {path}: {message}")]
    CreateDir { path: PathBuf, message: String },
    #[error("failed to read embedded terminal config at {path}: {message}")]
    ReadFile { path: PathBuf, message: String },
    #[error("failed to write embedded terminal config at {path}: {message}")]
    WriteFile { path: PathBuf, message: String },
}

pub fn embedded_terminal_config_paths() -> EmbeddedTerminalConfigPaths {
    config_paths_from_taskers_config_path(&taskers_paths::default_config_path())
}

pub fn load_or_initialize_embedded_terminal_config(
    legacy_appearance: Option<EmbeddedTerminalAppearance>,
) -> Result<EmbeddedTerminalConfigState, EmbeddedTerminalConfigError> {
    let paths = embedded_terminal_config_paths();
    fs::create_dir_all(&paths.dir).map_err(|error| EmbeddedTerminalConfigError::CreateDir {
        path: paths.dir.clone(),
        message: error.to_string(),
    })?;

    let mut startup_notes = vec![
        format!(
            "Embedded terminal managed config path {}",
            paths.base.display()
        ),
        format!(
            "Embedded terminal advanced override path {}",
            paths.override_file.display()
        ),
        "Embedded terminal config source order: base.conf < override.conf < Taskers invariants"
            .into(),
    ];

    if !paths.base.exists() {
        let managed = seed_managed_config(legacy_appearance);
        save_embedded_terminal_config_to_path(&paths.base, &managed)?;
        match legacy_appearance.unwrap_or(EmbeddedTerminalAppearance::Taskers) {
            EmbeddedTerminalAppearance::Ghostty => startup_notes.push(
                "Migrated legacy embedded_terminal_appearance=ghostty into Taskers-managed Ghostty defaults."
                    .into(),
            ),
            EmbeddedTerminalAppearance::Taskers => startup_notes.push(
                "Migrated legacy embedded terminal appearance into Taskers-managed defaults."
                    .into(),
            ),
        }
    }

    let mut load_notes = Vec::new();
    let managed = load_embedded_terminal_config_from_path(&paths.base, &mut load_notes)?;
    startup_notes.extend(load_notes);

    Ok(EmbeddedTerminalConfigState {
        paths,
        managed,
        startup_notes,
    })
}

pub fn save_embedded_terminal_config(
    managed: &EmbeddedTerminalConfig,
) -> Result<(), EmbeddedTerminalConfigError> {
    let paths = embedded_terminal_config_paths();
    fs::create_dir_all(&paths.dir).map_err(|error| EmbeddedTerminalConfigError::CreateDir {
        path: paths.dir.clone(),
        message: error.to_string(),
    })?;
    save_embedded_terminal_config_to_path(&paths.base, managed)
}

fn config_paths_from_taskers_config_path(
    taskers_config_path: &Path,
) -> EmbeddedTerminalConfigPaths {
    let config_dir = taskers_config_path
        .parent()
        .expect("taskers config path should have a parent");
    let dir = config_dir.join("ghostty");
    EmbeddedTerminalConfigPaths {
        base: dir.join("base.conf"),
        override_file: dir.join("override.conf"),
        dir,
    }
}

fn seed_managed_config(
    legacy_appearance: Option<EmbeddedTerminalAppearance>,
) -> EmbeddedTerminalConfig {
    match legacy_appearance.unwrap_or(EmbeddedTerminalAppearance::Taskers) {
        EmbeddedTerminalAppearance::Ghostty => EmbeddedTerminalConfig::default(),
        EmbeddedTerminalAppearance::Taskers => EmbeddedTerminalConfig {
            window_padding_x: Some("0".into()),
            window_padding_y: Some("0".into()),
            background_opacity: Some("0".into()),
            background_opacity_cells: OptionalBoolValue::Disabled,
            ..EmbeddedTerminalConfig::default()
        },
    }
}

fn load_embedded_terminal_config_from_path(
    path: &Path,
    startup_notes: &mut Vec<String>,
) -> Result<EmbeddedTerminalConfig, EmbeddedTerminalConfigError> {
    let contents =
        fs::read_to_string(path).map_err(|error| EmbeddedTerminalConfigError::ReadFile {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    let mut managed = EmbeddedTerminalConfig::default();
    let mut seen_keys = Vec::new();
    for line in contents.lines() {
        let Some((key, value)) = parse_assignment(line) else {
            continue;
        };

        if MANAGED_KEYS.contains(&key.as_str()) {
            if seen_keys.contains(&key) {
                startup_notes.push(format!(
                    "Embedded terminal config duplicate key '{key}' detected in base.conf; last value wins."
                ));
            } else {
                seen_keys.push(key.clone());
            }
            apply_managed_value(&mut managed, &key, &value);
        } else {
            startup_notes.push(format!(
                "Embedded terminal config key '{key}' is unmanaged in base.conf and will be preserved but not edited by the GUI."
            ));
        }
    }

    Ok(managed)
}

fn save_embedded_terminal_config_to_path(
    path: &Path,
    managed: &EmbeddedTerminalConfig,
) -> Result<(), EmbeddedTerminalConfigError> {
    let existing = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(EmbeddedTerminalConfigError::ReadFile {
                path: path.to_path_buf(),
                message: error.to_string(),
            });
        }
    };

    let preserved = strip_managed_lines(&existing);
    let rendered = render_base_file(managed, &preserved);
    fs::write(path, rendered).map_err(|error| EmbeddedTerminalConfigError::WriteFile {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn strip_managed_lines(existing: &str) -> Vec<String> {
    let mut preserved = Vec::new();
    let mut in_managed_block = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed == MANAGED_BEGIN {
            in_managed_block = true;
            continue;
        }
        if trimmed == MANAGED_END {
            in_managed_block = false;
            continue;
        }
        if in_managed_block {
            continue;
        }
        if MANAGED_HEADER
            .lines()
            .any(|managed_line| managed_line == line)
        {
            continue;
        }
        if let Some((key, _)) = parse_assignment(line)
            && MANAGED_KEYS.contains(&key.as_str())
        {
            continue;
        }
        preserved.push(line.to_string());
    }
    while preserved.first().is_some_and(|line| line.trim().is_empty()) {
        preserved.remove(0);
    }
    while preserved.last().is_some_and(|line| line.trim().is_empty()) {
        preserved.pop();
    }
    preserved
}

fn render_base_file(managed: &EmbeddedTerminalConfig, preserved: &[String]) -> String {
    let mut lines = Vec::new();
    lines.extend(MANAGED_HEADER.lines().map(str::to_string));
    if !preserved.is_empty() {
        lines.push(String::new());
        lines.extend(preserved.iter().cloned());
    }
    lines.push(String::new());
    lines.push(MANAGED_BEGIN.into());
    lines.extend(render_managed_entries(managed));
    lines.push(MANAGED_END.into());
    lines.push(String::new());
    lines.join("\n")
}

fn render_managed_entries(managed: &EmbeddedTerminalConfig) -> Vec<String> {
    let mut entries = Vec::new();
    push_entry(
        &mut entries,
        KEY_THEME,
        managed.theme.as_deref().map(quote_value),
    );
    push_entry(
        &mut entries,
        KEY_FONT_FAMILY,
        managed.font_family.as_deref().map(quote_value),
    );
    push_entry(
        &mut entries,
        KEY_FONT_SIZE,
        managed.font_size.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_WINDOW_PADDING_X,
        managed.window_padding_x.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_WINDOW_PADDING_Y,
        managed.window_padding_y.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_CURSOR_STYLE,
        managed.cursor_style.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_CURSOR_STYLE_BLINK,
        managed
            .cursor_style_blink
            .config_value()
            .map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_SCROLLBACK_LIMIT,
        managed.scrollback_limit.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_BACKGROUND_OPACITY,
        managed.background_opacity.as_deref().map(str::to_string),
    );
    push_entry(
        &mut entries,
        KEY_BACKGROUND_OPACITY_CELLS,
        managed
            .background_opacity_cells
            .config_value()
            .map(str::to_string),
    );
    entries
}

fn push_entry(entries: &mut Vec<String>, key: &str, value: Option<String>) {
    if let Some(value) = value.filter(|value| !value.trim().is_empty()) {
        entries.push(format!("{key} = {value}"));
    }
}

fn parse_assignment(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let without_comment = trimmed.split(" #").next().unwrap_or(trimmed).trim();
    let (key, value) = without_comment.split_once('=')?;
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    Some((key.to_string(), value.trim().to_string()))
}

fn apply_managed_value(managed: &mut EmbeddedTerminalConfig, key: &str, value: &str) {
    match key {
        KEY_THEME => managed.theme = normalize_quoted_value(value),
        KEY_FONT_FAMILY => managed.font_family = normalize_quoted_value(value),
        KEY_FONT_SIZE => managed.font_size = normalize_raw_value(value),
        KEY_WINDOW_PADDING_X => managed.window_padding_x = normalize_raw_value(value),
        KEY_WINDOW_PADDING_Y => managed.window_padding_y = normalize_raw_value(value),
        KEY_CURSOR_STYLE => managed.cursor_style = normalize_raw_value(value),
        KEY_CURSOR_STYLE_BLINK => {
            managed.cursor_style_blink = OptionalBoolValue::from_config_value(value)
        }
        KEY_SCROLLBACK_LIMIT => managed.scrollback_limit = normalize_raw_value(value),
        KEY_BACKGROUND_OPACITY => managed.background_opacity = normalize_raw_value(value),
        KEY_BACKGROUND_OPACITY_CELLS => {
            managed.background_opacity_cells = OptionalBoolValue::from_config_value(value)
        }
        _ => {}
    }
}

fn normalize_raw_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn normalize_quoted_value(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let trimmed = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(trimmed);
    Some(trimmed.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn quote_value(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddedTerminalAppearance, EmbeddedTerminalConfig, OptionalBoolValue,
        config_paths_from_taskers_config_path, load_embedded_terminal_config_from_path,
        render_base_file, save_embedded_terminal_config_to_path, seed_managed_config,
        strip_managed_lines,
    };
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn seed_taskers_defaults_match_taskers_appearance_contract() {
        let config = seed_managed_config(Some(EmbeddedTerminalAppearance::Taskers));
        assert_eq!(config.window_padding_x.as_deref(), Some("0"));
        assert_eq!(config.window_padding_y.as_deref(), Some("0"));
        assert_eq!(config.background_opacity.as_deref(), Some("0"));
        assert_eq!(config.background_opacity_cells, OptionalBoolValue::Disabled);
    }

    #[test]
    fn seed_ghostty_defaults_leave_managed_keys_unset() {
        let config = seed_managed_config(Some(EmbeddedTerminalAppearance::Ghostty));
        assert_eq!(config, EmbeddedTerminalConfig::default());
    }

    #[test]
    fn config_paths_live_under_taskers_config_dir() {
        let paths = config_paths_from_taskers_config_path(Path::new("/tmp/taskers/config.json"));
        assert_eq!(paths.base, Path::new("/tmp/taskers/ghostty/base.conf"));
        assert_eq!(
            paths.override_file,
            Path::new("/tmp/taskers/ghostty/override.conf")
        );
    }

    #[test]
    fn save_preserves_unmanaged_lines_and_rewrites_managed_block() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("base.conf");
        std::fs::write(
            &path,
            concat!(
                "# custom\n",
                "theme = \"Old Theme\"\n",
                "font-size = 14\n",
                "confirm-close-surface = false\n",
            ),
        )
        .expect("write base");

        let config = EmbeddedTerminalConfig {
            theme: Some("Catppuccin Mocha".into()),
            font_size: Some("16".into()),
            cursor_style_blink: OptionalBoolValue::Enabled,
            ..EmbeddedTerminalConfig::default()
        };
        save_embedded_terminal_config_to_path(&path, &config).expect("save config");
        let saved = std::fs::read_to_string(&path).expect("read saved");

        assert!(saved.contains("confirm-close-surface = false"));
        assert!(saved.contains("theme = \"Catppuccin Mocha\""));
        assert!(saved.contains("font-size = 16"));
        assert!(saved.contains("cursor-style-blink = true"));
        assert!(!saved.contains("theme = \"Old Theme\""));
    }

    #[test]
    fn load_reports_unmanaged_base_keys_and_duplicate_managed_keys() {
        let temp = tempdir().expect("tempdir");
        let path = temp.path().join("base.conf");
        std::fs::write(
            &path,
            concat!(
                "font-size = 14\n",
                "cursor-style = block\n",
                "font-size = 18\n",
                "confirm-close-surface = false\n",
            ),
        )
        .expect("write base");

        let mut notes = Vec::new();
        let config = load_embedded_terminal_config_from_path(&path, &mut notes).expect("load");

        assert_eq!(config.font_size.as_deref(), Some("18"));
        assert_eq!(config.cursor_style.as_deref(), Some("block"));
        assert!(
            notes
                .iter()
                .any(|note| note.contains("duplicate key 'font-size'"))
        );
        assert!(
            notes
                .iter()
                .any(|note| note.contains("confirm-close-surface"))
        );
    }

    #[test]
    fn strip_managed_lines_removes_managed_keys_and_generated_block() {
        let preserved = strip_managed_lines(&render_base_file(
            &EmbeddedTerminalConfig {
                theme: Some("Tokyo Night".into()),
                ..EmbeddedTerminalConfig::default()
            },
            &["confirm-close-surface = false".into()],
        ));
        assert_eq!(preserved, vec!["confirm-close-surface = false".to_string()]);
    }
}
