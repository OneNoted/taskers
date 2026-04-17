use std::ffi::{CString, c_char};

#[cfg(taskers_ghostty_bridge)]
use std::{
    ffi::{c_int, c_void},
    ptr::NonNull,
    slice,
};

use gtk::Widget;
#[cfg(taskers_ghostty_bridge)]
use gtk::glib::translate::from_glib_full;
#[cfg(taskers_ghostty_bridge)]
use gtk::prelude::ObjectType;
use thiserror::Error;

use crate::backend::{GhosttyHostOptions, SurfaceDescriptor};
use crate::runtime::configure_runtime_environment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyBridgeInfo {
    pub version: String,
    pub build_id: String,
}

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
    #[error("failed to read text from ghostty surface")]
    SurfaceReadText,
    #[error("failed to write text to ghostty surface")]
    SurfaceWriteText,
    #[error("surface metadata contains NUL bytes: {0}")]
    InvalidString(&'static str),
}

#[cfg(taskers_ghostty_bridge)]
pub struct GhosttyHost {
    raw: NonNull<taskers_ghostty_host_t>,
}

#[cfg(not(taskers_ghostty_bridge))]
pub struct GhosttyHost;

impl GhosttyHost {
    pub fn new() -> Result<Self, GhosttyError> {
        Self::new_with_options(&GhosttyHostOptions::default())
    }

    pub fn new_with_options(options: &GhosttyHostOptions) -> Result<Self, GhosttyError> {
        configure_runtime_environment();

        #[cfg(taskers_ghostty_bridge)]
        unsafe {
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
            let base_config_path = options
                .base_config_path
                .as_deref()
                .map(|value| {
                    CString::new(value).map_err(|_| GhosttyError::InvalidString("base_config_path"))
                })
                .transpose()?;
            let override_config_path = options
                .override_config_path
                .as_deref()
                .map(|value| {
                    CString::new(value)
                        .map_err(|_| GhosttyError::InvalidString("override_config_path"))
                })
                .transpose()?;
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
                base_config_path: base_config_path
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
                override_config_path: override_config_path
                    .as_ref()
                    .map_or(std::ptr::null(), |value| value.as_ptr()),
            };

            let raw = taskers_ghostty_host_new(&host_options);
            let raw = NonNull::new(raw).ok_or(GhosttyError::HostInit)?;
            Ok(Self { raw })
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
            let ok = taskers_ghostty_host_tick(self.raw.as_ptr());
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

    pub fn bridge_info(&self) -> GhosttyBridgeInfo {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let version = std::ffi::CStr::from_ptr(taskers_ghostty_host_version())
                .to_string_lossy()
                .into_owned();
            let build_id = std::ffi::CStr::from_ptr(taskers_ghostty_host_build_id())
                .to_string_lossy()
                .into_owned();
            GhosttyBridgeInfo { version, build_id }
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            GhosttyBridgeInfo {
                version: "unavailable".into(),
                build_id: "unavailable".into(),
            }
        }
    }

    pub fn begin_shutdown(&self) {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            taskers_ghostty_host_begin_shutdown(self.raw.as_ptr());
        }
    }

    pub fn surface_count(&self) -> usize {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            taskers_ghostty_host_surface_count(self.raw.as_ptr())
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            0
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

            let widget = taskers_ghostty_surface_new(self.raw.as_ptr(), &options);
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
            let ok = taskers_ghostty_surface_grab_focus(widget.as_ptr().cast());
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

    pub fn destroy_surface(&self, widget: &Widget) {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            taskers_ghostty_surface_destroy(widget.as_ptr().cast());
        }
    }

    pub fn surface_has_selection(&self, widget: &Widget) -> Result<bool, GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            Ok(taskers_ghostty_surface_has_selection(widget.as_ptr().cast()) != 0)
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = widget;
            Err(GhosttyError::Unavailable)
        }
    }

    pub fn read_surface_text(&self, widget: &Widget) -> Result<String, GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let mut text = taskers_ghostty_text_s::default();
            let ok = taskers_ghostty_surface_read_all_text(widget.as_ptr().cast(), &mut text);
            if ok == 0 {
                return Err(GhosttyError::SurfaceReadText);
            }

            let bytes = if text.text.is_null() || text.text_len == 0 {
                &[]
            } else {
                slice::from_raw_parts(text.text.cast::<u8>(), text.text_len)
            };
            let output = String::from_utf8_lossy(bytes).into_owned();
            taskers_ghostty_surface_free_text(&mut text);
            Ok(output)
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = widget;
            Err(GhosttyError::Unavailable)
        }
    }

    pub fn send_surface_text(&self, widget: &Widget, text: &str) -> Result<(), GhosttyError> {
        #[cfg(taskers_ghostty_bridge)]
        unsafe {
            let text =
                CString::new(text).map_err(|_| GhosttyError::InvalidString("surface_text"))?;
            let ok = taskers_ghostty_surface_send_text(
                widget.as_ptr().cast(),
                text.as_ptr(),
                text.as_bytes().len(),
            );
            if ok == 0 {
                Err(GhosttyError::SurfaceWriteText)
            } else {
                Ok(())
            }
        }

        #[cfg(not(taskers_ghostty_bridge))]
        {
            let _ = (widget, text);
            Err(GhosttyError::Unavailable)
        }
    }
}

#[cfg(taskers_ghostty_bridge)]
impl Drop for GhosttyHost {
    fn drop(&mut self) {
        unsafe {
            taskers_ghostty_host_free(self.raw.as_ptr());
        }
    }
}

#[cfg(taskers_ghostty_bridge)]
unsafe extern "C" {
    fn taskers_ghostty_host_new(
        options: *const taskers_ghostty_host_options_s,
    ) -> *mut taskers_ghostty_host_t;
    fn taskers_ghostty_host_free(host: *mut taskers_ghostty_host_t);
    fn taskers_ghostty_host_version() -> *const c_char;
    fn taskers_ghostty_host_build_id() -> *const c_char;
    fn taskers_ghostty_host_begin_shutdown(host: *mut taskers_ghostty_host_t);
    fn taskers_ghostty_host_surface_count(host: *mut taskers_ghostty_host_t) -> usize;
    fn taskers_ghostty_host_tick(host: *mut taskers_ghostty_host_t) -> c_int;
    fn taskers_ghostty_surface_new(
        host: *mut taskers_ghostty_host_t,
        options: *const taskers_ghostty_surface_options_s,
    ) -> *mut c_void;
    fn taskers_ghostty_surface_destroy(widget: *mut c_void);
    fn taskers_ghostty_surface_grab_focus(widget: *mut c_void) -> c_int;
    fn taskers_ghostty_surface_has_selection(widget: *mut c_void) -> c_int;
    fn taskers_ghostty_surface_send_text(
        widget: *mut c_void,
        text: *const c_char,
        len: usize,
    ) -> c_int;
    fn taskers_ghostty_surface_read_all_text(
        widget: *mut c_void,
        text: *mut taskers_ghostty_text_s,
    ) -> c_int;
    fn taskers_ghostty_surface_free_text(text: *mut taskers_ghostty_text_s);
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
    base_config_path: *const c_char,
    override_config_path: *const c_char,
}

#[cfg(taskers_ghostty_bridge)]
#[repr(C)]
struct taskers_ghostty_surface_options_s {
    working_directory: *const c_char,
    title: *const c_char,
    env_entries: *const *const c_char,
    env_count: usize,
}

#[cfg(taskers_ghostty_bridge)]
#[repr(C)]
#[derive(Default)]
struct taskers_ghostty_text_s {
    text: *const c_char,
    text_len: usize,
}
