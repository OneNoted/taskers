pub mod backend;
#[cfg(target_os = "linux")]
pub mod bridge;
pub mod embedded_config;
pub mod runtime;

pub use backend::{
    AdapterError, BackendAvailability, BackendChoice, BackendProbe, DefaultBackend,
    EmbeddedTerminalAppearance, GhosttyGtkHostOptions, GhosttyGtkSurfaceDescriptor,
    TerminalBackend,
};
#[cfg(target_os = "linux")]
pub use bridge::{
    GHOSTTY_GTK_PROPERTY_CHILD_EXITED, GHOSTTY_GTK_PROPERTY_PWD, GHOSTTY_GTK_PROPERTY_TITLE,
    GhosttyGtkError, GhosttyGtkHost, GhosttyGtkInfo,
};
pub use embedded_config::{
    EmbeddedTerminalConfig, EmbeddedTerminalConfigError, EmbeddedTerminalConfigPaths,
    EmbeddedTerminalConfigState, OptionalBoolValue, embedded_terminal_config_paths,
    load_or_initialize_embedded_terminal_config, save_embedded_terminal_config,
};
pub use runtime::{
    RuntimeBootstrap, RuntimeBootstrapError, configure_runtime_environment,
    ensure_runtime_installed, runtime_terminfo_dir,
};

#[cfg(target_os = "linux")]
pub type GhosttyHost = GhosttyGtkHost;
#[cfg(target_os = "linux")]
pub type GhosttyBridgeInfo = GhosttyGtkInfo;
#[cfg(target_os = "linux")]
pub type GhosttyError = GhosttyGtkError;
pub type GhosttyHostOptions = GhosttyGtkHostOptions;
pub type SurfaceDescriptor = GhosttyGtkSurfaceDescriptor;
