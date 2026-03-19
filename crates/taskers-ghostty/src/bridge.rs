use std::{
    ffi::{CString, c_char},
    path::PathBuf,
};

#[cfg(taskers_ghostty_bridge)]
use std::{
    ffi::{c_int, c_void},
    ptr::NonNull,
};

use gtk::Widget;
#[cfg(taskers_ghostty_bridge)]
use gtk::glib::translate::from_glib_full;
#[cfg(taskers_ghostty_bridge)]
use gtk::prelude::ObjectType;
#[cfg(taskers_ghostty_bridge)]
use libloading::Library;
use thiserror::Error;

use crate::backend::{GhosttyHostOptions, SurfaceDescriptor};
use crate::runtime::{configure_runtime_environment, runtime_bridge_path};

#[derive(Debug, Error)]
pub enum GhosttyError {
    #[error("ghostty bridge is unavailable in this build")]
    Unavailable,
    #[error("failed to initialize ghostty host")]
    HostInit,
    #[error("failed to tick ghostty host")]
    Tick,
    #[error("failed to create ghostty surface")]
    SurfaceInit,
    #[error("surface metadata contains NUL bytes: {0}")]
    InvalidString(&'static str),
    #[error("failed to load ghostty bridge library from {path}: {message}")]
    LibraryLoad { path: PathBuf, message: String },
    #[error("ghostty bridge library path is unavailable")]
    LibraryPathUnavailable,
}

#[cfg(taskers_ghostty_bridge)]
pub struct GhosttyHost {
    bridge: GhosttyBridgeLibrary,
    raw: NonNull<taskers_ghostty_host_t>,
}

#[cfg(not(taskers_ghostty_bridge))]
pub struct GhosttyHost;

#[cfg(taskers_ghostty_bridge)]
struct GhosttyBridgeLibrary {
    _library: Library,
    host_new:
        unsafe extern "C" fn(*const taskers_ghostty_host_options_s) -> *mut taskers_ghostty_host_t,
    host_free: unsafe extern "C" fn(*mut taskers_ghostty_host_t),
    host_tick: unsafe extern "C" fn(*mut taskers_ghostty_host_t) -> c_int,
    surface_new: unsafe extern "C" fn(
        *mut taskers_ghostty_host_t,
        *const taskers_ghostty_surface_options_s,
    ) -> *mut c_void,
    surface_grab_focus: unsafe extern "C" fn(*mut c_void) -> c_int,
}

impl GhosttyHost {
    pub fn new() -> Result<Self, GhosttyError> {
        Self::new_with_options(&GhosttyHostOptions::default())
    }

    pub fn new_with_options(options: &GhosttyHostOptions) -> Result<Self, GhosttyError> {
        configure_runtime_environment();

        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let bridge = load_bridge_library()?;
            let command_argv = options
                .command_argv
                .iter()
                .map(|value| {
                    CString::new(value.as_str())
                        .map_err(|_| GhosttyError::InvalidString("command_argv"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let command_argv_ptrs = command_argv
                .iter()
                .map(|value| value.as_ptr())
                .collect::<Vec<_>>();
            let env_entries = options
                .env
                .iter()
                .map(|(key, value)| {
                    CString::new(format!("{key}={value}"))
                        .map_err(|_| GhosttyError::InvalidString("env"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let env_entry_ptrs = env_entries
                .iter()
                .map(|value| value.as_ptr())
                .collect::<Vec<_>>();
            let host_options = taskers_ghostty_host_options_s {
                command_argv: if command_argv_ptrs.is_empty() {
                    std::ptr::null()
                } else {
                    command_argv_ptrs.as_ptr()
                },
                command_argc: command_argv_ptrs.len(),
                env_entries: if env_entry_ptrs.is_empty() {
                    std::ptr::null()
                } else {
                    env_entry_ptrs.as_ptr()
                },
                env_count: env_entry_ptrs.len(),
            };

            let raw = (bridge.host_new)(&host_options);
            let raw = NonNull::new(raw).ok_or(GhosttyError::HostInit)?;
            Ok(Self { bridge, raw })
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = options;
            Err(GhosttyError::Unavailable)
        }
    }

    pub fn tick(&self) -> Result<(), GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let ok = (self.bridge.host_tick)(self.raw.as_ptr());
            if ok == 0 {
                Err(GhosttyError::Tick)
            } else {
                Ok(())
            }
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            Err(GhosttyError::Unavailable)
        }
    }

    pub fn create_surface(&self, descriptor: &SurfaceDescriptor) -> Result<Widget, GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let cwd = descriptor
                .cwd
                .as_deref()
                .map(|value| CString::new(value).map_err(|_| GhosttyError::InvalidString("cwd")))
                .transpose()?;
            let title = descriptor
                .title
                .as_deref()
                .map(|value| CString::new(value).map_err(|_| GhosttyError::InvalidString("title")))
                .transpose()?;
            let env_entries = descriptor
                .env
                .iter()
                .map(|(key, value)| {
                    CString::new(format!("{key}={value}"))
                        .map_err(|_| GhosttyError::InvalidString("env"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let env_entry_ptrs = env_entries
                .iter()
                .map(|value| value.as_ptr())
                .collect::<Vec<_>>();

            let options = taskers_ghostty_surface_options_s {
                working_directory: cwd
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                title: title
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                env_entries: if env_entry_ptrs.is_empty() {
                    std::ptr::null()
                } else {
                    env_entry_ptrs.as_ptr()
                },
                env_count: env_entry_ptrs.len(),
            };

            let widget = (self.bridge.surface_new)(self.raw.as_ptr(), &options);
            if widget.is_null() {
                return Err(GhosttyError::SurfaceInit);
            }

            Ok(from_glib_full(widget.cast()))
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = descriptor;
            Err(GhosttyError::Unavailable)
        }
    }

    pub fn focus_surface(&self, widget: &Widget) -> Result<(), GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let ok = (self.bridge.surface_grab_focus)(widget.as_ptr().cast());
            if ok == 0 {
                Err(GhosttyError::SurfaceInit)
            } else {
                Ok(())
            }
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = widget;
            Err(GhosttyError::Unavailable)
        }
    }
}

#[cfg(taskers_ghostty_bridge)]
impl Drop for GhosttyHost {
    fn drop(&mut self) {
        unsafe {
            (self.bridge.host_free)(self.raw.as_ptr());
        }
    }
}

#[cfg(taskers_ghostty_bridge)]
fn load_bridge_library() -> Result<GhosttyBridgeLibrary, GhosttyError> {
    let path = runtime_bridge_path().ok_or(GhosttyError::LibraryPathUnavailable)?;
    let library = unsafe {
        Library::new(&path).map_err(|error| GhosttyError::LibraryLoad {
            path: path.clone(),
            message: error.to_string(),
        })?
    };

    unsafe {
        let host_new = *library
            .get::<unsafe extern "C" fn(
                *const taskers_ghostty_host_options_s,
            ) -> *mut taskers_ghostty_host_t>(
                b"taskers_ghostty_host_new\0",
            )
            .map_err(|error| GhosttyError::LibraryLoad {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let host_free = *library
            .get::<unsafe extern "C" fn(*mut taskers_ghostty_host_t)>(
                b"taskers_ghostty_host_free\0",
            )
            .map_err(|error| GhosttyError::LibraryLoad {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let host_tick = *library
            .get::<unsafe extern "C" fn(*mut taskers_ghostty_host_t) -> c_int>(
                b"taskers_ghostty_host_tick\0",
            )
            .map_err(|error| GhosttyError::LibraryLoad {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let surface_new = *library
            .get::<unsafe extern "C" fn(
                *mut taskers_ghostty_host_t,
                *const taskers_ghostty_surface_options_s,
            ) -> *mut c_void>(b"taskers_ghostty_surface_new\0")
            .map_err(|error| GhosttyError::LibraryLoad {
                path: path.clone(),
                message: error.to_string(),
            })?;
        let surface_grab_focus = *library
            .get::<unsafe extern "C" fn(*mut c_void) -> c_int>(
                b"taskers_ghostty_surface_grab_focus\0",
            )
            .map_err(|error| GhosttyError::LibraryLoad {
                path: path.clone(),
                message: error.to_string(),
            })?;

        Ok(GhosttyBridgeLibrary {
            _library: library,
            host_new,
            host_free,
            host_tick,
            surface_new,
            surface_grab_focus,
        })
    }
}

#[cfg(taskers_ghostty_bridge)]
#[repr(C)]
struct taskers_ghostty_host_t {
    _private: [u8; 0],
}

#[cfg(taskers_ghostty_bridge)]
#[repr(C)]
struct taskers_ghostty_host_options_s {
    command_argv: *const *const c_char,
    command_argc: usize,
    env_entries: *const *const c_char,
    env_count: usize,
}

#[cfg(taskers_ghostty_bridge)]
#[repr(C)]
struct taskers_ghostty_surface_options_s {
    working_directory: *const c_char,
    title: *const c_char,
    env_entries: *const *const c_char,
    env_count: usize,
}
