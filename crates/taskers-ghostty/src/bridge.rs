use std::{
    ffi::{CString, c_char, c_int, c_void},
    path::PathBuf,
    ptr::NonNull,
};

use gtk::{Widget, glib::translate::from_glib_full};
use libloading::Library;
use thiserror::Error;

use crate::backend::SurfaceDescriptor;

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
    host_new: unsafe extern "C" fn() -> *mut taskers_ghostty_host_t,
    host_free: unsafe extern "C" fn(*mut taskers_ghostty_host_t),
    host_tick: unsafe extern "C" fn(*mut taskers_ghostty_host_t) -> c_int,
    surface_new: unsafe extern "C" fn(
        *mut taskers_ghostty_host_t,
        *const taskers_ghostty_surface_options_s,
    ) -> *mut c_void,
}

impl GhosttyHost {
    pub fn new() -> Result<Self, GhosttyError> {
        configure_runtime_environment();

        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let bridge = load_bridge_library()?;
            let raw = (bridge.host_new)();
            let raw = NonNull::new(raw).ok_or(GhosttyError::HostInit)?;
            Ok(Self { bridge, raw })
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
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

            let options = taskers_ghostty_surface_options_s {
                working_directory: cwd
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                title: title
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
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
}

#[cfg(taskers_ghostty_bridge)]
impl Drop for GhosttyHost {
    fn drop(&mut self) {
        unsafe {
            (self.bridge.host_free)(self.raw.as_ptr());
        }
    }
}

pub fn configure_runtime_environment() {
    if std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some() {
        return;
    }

    if let Some(path) = installed_runtime_dir().filter(|path| path.exists()) {
        unsafe {
            std::env::set_var("GHOSTTY_RESOURCES_DIR", &path);
        }
        return;
    }

    if let Some(path) = option_env!("TASKERS_GHOSTTY_BUILD_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        unsafe {
            std::env::set_var("GHOSTTY_RESOURCES_DIR", &path);
        }
    }
}

pub fn runtime_resources_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("GHOSTTY_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = installed_runtime_dir().filter(|path| path.exists()) {
        return Some(path);
    }

    option_env!("TASKERS_GHOSTTY_BUILD_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

pub fn runtime_bridge_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TASKERS_GHOSTTY_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = installed_runtime_dir()
        .map(|root| root.join("lib").join("libtaskers_ghostty_bridge.so"))
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    option_env!("TASKERS_GHOSTTY_BUILD_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn installed_runtime_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR").map(PathBuf::from) {
        return Some(path);
    }

    if let Some(path) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .map(|path| path.join("taskers").join("ghostty"))
    {
        return Some(path);
    }

    std::env::var_os("HOME").map(PathBuf::from).map(|path| {
        path.join(".local")
            .join("share")
            .join("taskers")
            .join("ghostty")
    })
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
            .get::<unsafe extern "C" fn() -> *mut taskers_ghostty_host_t>(
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

        Ok(GhosttyBridgeLibrary {
            _library: library,
            host_new,
            host_free,
            host_tick,
            surface_new,
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
struct taskers_ghostty_surface_options_s {
    working_directory: *const c_char,
    title: *const c_char,
}
