use crate::runtime::{runtime_bridge_path, runtime_resources_dir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use taskers_domain::PaneKind;
use thiserror::Error;

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
    pub command_argv: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
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
    use super::{BackendAvailability, BackendChoice, DefaultBackend, TerminalBackend};

    #[test]
    fn auto_probe_matches_runtime_availability() {
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
        unsafe { std::env::set_var("TASKERS_TERMINAL_BACKEND", "ghostty-embedded") };
        let probe = DefaultBackend::probe(BackendChoice::Mock);
        unsafe { std::env::remove_var("TASKERS_TERMINAL_BACKEND") };
        assert_eq!(probe.selected, BackendChoice::GhosttyEmbedded);
    }
}
