use crate::runtime::{runtime_bridge_path, runtime_resources_dir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use taskers_domain::{BrowserProfileMode, PaneKind};
use taskers_runtime::ShellLaunchSpec;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddedTerminalAppearance {
    #[default]
    Taskers,
    Ghostty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendChoice {
    Auto,
    Ghostty,
    GhosttyEmbedded,
    Mock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendAvailability {
    Ready,
    Fallback,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendProbe {
    pub requested: BackendChoice,
    pub selected: BackendChoice,
    pub availability: BackendAvailability,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceDescriptor {
    pub cols: u16,
    pub rows: u16,
    pub kind: PaneKind,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub browser_profile_mode: BrowserProfileMode,
    #[serde(default)]
    pub command_argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhosttyHostOptions {
    #[serde(default)]
    pub command_argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub embedded_terminal_appearance: EmbeddedTerminalAppearance,
}

impl GhosttyHostOptions {
    pub fn from_shell_launch(shell_launch: &ShellLaunchSpec) -> Self {
        let mut env = BTreeMap::new();
        env.extend(shell_launch.env.clone());
        let command_argv = if shell_launch
            .program
            .file_name()
            .and_then(|value| value.to_str())
            == Some("taskers-shell-wrapper.sh")
        {
            vec![shell_launch.program.display().to_string()]
        } else {
            shell_launch.program_and_args()
        };
        Self {
            command_argv,
            env,
            embedded_terminal_appearance: EmbeddedTerminalAppearance::Taskers,
        }
    }

    pub fn with_embedded_terminal_appearance(
        mut self,
        embedded_terminal_appearance: EmbeddedTerminalAppearance,
    ) -> Self {
        self.embedded_terminal_appearance = embedded_terminal_appearance;
        self
    }
}

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("terminal backend is unavailable: {0}")]
    Unavailable(String),
    #[error("terminal backend initialization failed: {0}")]
    Initialization(String),
}

pub trait TerminalBackend {
    fn probe(requested: BackendChoice) -> BackendProbe;
}

pub struct DefaultBackend;

impl TerminalBackend for DefaultBackend {
    fn probe(requested: BackendChoice) -> BackendProbe {
        let env_override = std::env::var("TASKERS_TERMINAL_BACKEND").ok();
        let requested = match env_override.as_deref() {
            Some("ghostty") => BackendChoice::Ghostty,
            Some("ghostty_embedded") | Some("ghostty-embedded") => BackendChoice::GhosttyEmbedded,
            Some("mock") => BackendChoice::Mock,
            _ => requested,
        };

        match requested {
            BackendChoice::Auto => auto_probe(requested),
            BackendChoice::Mock => BackendProbe {
                requested,
                selected: BackendChoice::Mock,
                availability: BackendAvailability::Fallback,
                notes: "Using placeholder terminal surfaces.".into(),
            },
            BackendChoice::GhosttyEmbedded => BackendProbe {
                requested,
                selected: BackendChoice::GhosttyEmbedded,
                availability: embedded_ghostty_availability(),
                notes: embedded_ghostty_notes(),
            },
            BackendChoice::Ghostty => BackendProbe {
                requested,
                selected: BackendChoice::Ghostty,
                availability: ghostty_availability(),
                notes: ghostty_notes(),
            },
        }
    }
}

fn auto_probe(requested: BackendChoice) -> BackendProbe {
    let availability = ghostty_availability();
    if matches!(availability, BackendAvailability::Ready) {
        BackendProbe {
            requested,
            selected: BackendChoice::Ghostty,
            availability,
            notes: ghostty_notes(),
        }
    } else {
        BackendProbe {
            requested,
            selected: BackendChoice::Mock,
            availability: BackendAvailability::Fallback,
            notes: "Ghostty bridge unavailable, using placeholder terminal surfaces.".into(),
        }
    }
}

fn ghostty_availability() -> BackendAvailability {
    #[cfg(all(target_os = "linux", taskers_ghostty_bridge))]
    {
        if runtime_bridge_path().is_some() {
            BackendAvailability::Ready
        } else {
            BackendAvailability::Unavailable
        }
    }

    #[cfg(not(all(target_os = "linux", taskers_ghostty_bridge)))]
    {
        BackendAvailability::Unavailable
    }
}

fn embedded_ghostty_availability() -> BackendAvailability {
    #[cfg(target_os = "macos")]
    {
        BackendAvailability::Ready
    }

    #[cfg(not(target_os = "macos"))]
    {
        BackendAvailability::Unavailable
    }
}

fn ghostty_notes() -> String {
    let mut notes = String::from("Ghostty GTK bridge compiled in.");
    if let Some(path) = runtime_bridge_path() {
        notes.push_str(" Bridge: ");
        notes.push_str(&path.display().to_string());
    } else {
        notes.push_str(" Bridge library not found.");
    }
    if let Some(path) = runtime_resources_dir() {
        notes.push_str(" Resources: ");
        notes.push_str(&path.display().to_string());
    }
    notes
}

fn embedded_ghostty_notes() -> String {
    String::from("Embedded Ghostty surfaces require the native macOS host.")
}

#[cfg(test)]
mod tests {
    use super::{
        BackendAvailability, BackendChoice, DefaultBackend, EmbeddedTerminalAppearance,
        GhosttyHostOptions, TerminalBackend,
    };
    use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};
    use taskers_runtime::ShellLaunchSpec;

    static BACKEND_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn auto_probe_matches_runtime_availability() {
        let _guard = BACKEND_ENV_LOCK.lock().expect("env lock");
        unsafe { std::env::remove_var("TASKERS_TERMINAL_BACKEND") };
        let probe = DefaultBackend::probe(BackendChoice::Auto);
        match probe.availability {
            BackendAvailability::Ready => assert_eq!(probe.selected, BackendChoice::Ghostty),
            BackendAvailability::Fallback | BackendAvailability::Unavailable => {
                assert_eq!(probe.selected, BackendChoice::Mock);
            }
        }
    }

    #[test]
    fn embedded_probe_stays_explicit() {
        let probe = DefaultBackend::probe(BackendChoice::GhosttyEmbedded);
        assert_eq!(probe.selected, BackendChoice::GhosttyEmbedded);

        #[cfg(target_os = "macos")]
        assert_eq!(probe.availability, BackendAvailability::Ready);

        #[cfg(not(target_os = "macos"))]
        assert_eq!(probe.availability, BackendAvailability::Unavailable);
    }

    #[test]
    fn env_override_accepts_hyphenated_embedded_backend() {
        let _guard = BACKEND_ENV_LOCK.lock().expect("env lock");
        unsafe { std::env::set_var("TASKERS_TERMINAL_BACKEND", "ghostty-embedded") };
        let probe = DefaultBackend::probe(BackendChoice::Mock);
        unsafe { std::env::remove_var("TASKERS_TERMINAL_BACKEND") };
        assert_eq!(probe.selected, BackendChoice::GhosttyEmbedded);
    }

    #[test]
    fn host_options_follow_shell_launch_contract() {
        let mut env = BTreeMap::new();
        env.insert("TASKERS_SOCKET".into(), "/tmp/taskers.sock".into());
        let shell_launch = ShellLaunchSpec {
            program: PathBuf::from("/bin/zsh"),
            args: vec!["-i".into()],
            env,
        };

        let options = GhosttyHostOptions::from_shell_launch(&shell_launch);

        assert_eq!(options.command_argv, vec!["/bin/zsh", "-i"]);
        assert_eq!(
            options.env.get("TASKERS_SOCKET").map(String::as_str),
            Some("/tmp/taskers.sock")
        );
        assert_eq!(
            options.embedded_terminal_appearance,
            EmbeddedTerminalAppearance::Taskers
        );
    }

    #[test]
    fn host_options_collapse_wrapper_shell_args_for_embedded_ghostty() {
        let mut env = BTreeMap::new();
        env.insert("TASKERS_REAL_SHELL".into(), "/usr/bin/fish".into());
        env.insert("TASKERS_SHELL_PROFILE".into(), "default".into());
        let shell_launch = ShellLaunchSpec {
            program: PathBuf::from("/tmp/taskers-runtime/taskers-shell-wrapper.sh"),
            args: vec![
                "--interactive".into(),
                "--init-command".into(),
                r#"source "$TASKERS_SHELL_INTEGRATION_DIR/taskers-hooks.fish""#.into(),
            ],
            env,
        };

        let options = GhosttyHostOptions::from_shell_launch(&shell_launch);

        assert_eq!(
            options.command_argv,
            vec!["/tmp/taskers-runtime/taskers-shell-wrapper.sh"]
        );
        assert_eq!(
            options.env.get("TASKERS_REAL_SHELL").map(String::as_str),
            Some("/usr/bin/fish")
        );
    }

    #[test]
    fn host_options_allow_overriding_embedded_terminal_appearance() {
        let options = GhosttyHostOptions::default()
            .with_embedded_terminal_appearance(EmbeddedTerminalAppearance::Ghostty);
        assert_eq!(
            options.embedded_terminal_appearance,
            EmbeddedTerminalAppearance::Ghostty
        );
    }
}
