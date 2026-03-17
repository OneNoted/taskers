pub mod backend;
#[cfg(target_os = "linux")]
pub mod bridge;
pub mod runtime;

pub use backend::{
    AdapterError, BackendAvailability, BackendChoice, BackendProbe, DefaultBackend,
    SurfaceDescriptor, TerminalBackend,
};
#[cfg(target_os = "linux")]
pub use bridge::{GhosttyError, GhosttyHost};
pub use runtime::{
    RuntimeBootstrap, RuntimeBootstrapError, configure_runtime_environment,
    ensure_runtime_installed,
};
