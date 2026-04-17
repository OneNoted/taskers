use std::ffi::{CString, c_char};

#[cfg(ghostty_gtk_bridge)]
use std::{ffi::c_int, ptr::NonNull, slice};

use gtk::Widget;
#[cfg(ghostty_gtk_bridge)]
use gtk::glib::translate::from_glib_full;
#[cfg(ghostty_gtk_bridge)]
use gtk::prelude::ObjectType;
use thiserror::Error;

use crate::backend::{GhosttyGtkHostOptions, GhosttyGtkSurfaceDescriptor};
use crate::runtime::configure_runtime_environment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyGtkInfo {
    pub version: String,
    pub build_id: String,
}

pub const GHOSTTY_GTK_PROPERTY_TITLE: &str = "title";
pub const GHOSTTY_GTK_PROPERTY_PWD: &str = "pwd";
pub const GHOSTTY_GTK_PROPERTY_CHILD_EXITED: &str = "child-exited";

#[derive(Debug, Error)]
pub enum GhosttyGtkError {
    #[error("ghostty gtk host is unavailable in this build")]
    Unavailable,
    #[error("failed to initialize ghostty gtk host")]
    HostInit,
    #[error("failed to tick ghostty gtk host")]
    Tick,
    #[error("failed to create ghostty gtk surface")]
    SurfaceInit,
    #[error("failed to read text from ghostty gtk surface")]
    SurfaceReadText,
    #[error("failed to write text to ghostty gtk surface")]
    SurfaceWriteText,
    #[error("surface metadata contains NUL bytes: {0}")]
    InvalidString(&'static str),
}

#[cfg(ghostty_gtk_bridge)]
pub struct GhosttyGtkHost {
    raw: NonNull<ghostty_gtk_host_t>,
}

#[cfg(not(ghostty_gtk_bridge))]
pub struct GhosttyGtkHost;

impl GhosttyGtkHost {
    pub fn new() -> Result<Self, GhosttyGtkError> {
        Self::new_with_options(&GhosttyGtkHostOptions::default())
    }

    pub fn new_with_options(options: &GhosttyGtkHostOptions) -> Result<Self, GhosttyGtkError> {
        configure_runtime_environment();

        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let command_argv = options
                .command_argv
                .iter()
                .map(|value| {
                    CString::new(value.as_str())
                        .map_err(|_| GhosttyGtkError::InvalidString("command_argv"))
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
                        .map_err(|_| GhosttyGtkError::InvalidString("env"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let base_config_path = options
                .base_config_path
                .as_deref()
                .map(|value| {
                    CString::new(value)
                        .map_err(|_| GhosttyGtkError::InvalidString("base_config_path"))
                })
                .transpose()?;
            let override_config_path = options
                .override_config_path
                .as_deref()
                .map(|value| {
                    CString::new(value)
                        .map_err(|_| GhosttyGtkError::InvalidString("override_config_path"))
                })
                .transpose()?;
            let env_entry_ptrs = env_entries
                .iter()
                .map(|value| value.as_ptr())
                .collect::<Vec<_>>();
            let host_options = ghostty_gtk_host_options_s {
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

            let raw = ghostty_gtk_host_new(&host_options);
            let raw = NonNull::new(raw).ok_or(GhosttyGtkError::HostInit)?;
            Ok(Self { raw })
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = options;
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn tick(&self) -> Result<(), GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let ok = ghostty_gtk_host_tick(self.raw.as_ptr());
            if ok == 0 {
                Err(GhosttyGtkError::Tick)
            } else {
                Ok(())
            }
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn bridge_info(&self) -> GhosttyGtkInfo {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let version = std::ffi::CStr::from_ptr(ghostty_gtk_host_version())
                .to_string_lossy()
                .into_owned();
            let build_id = std::ffi::CStr::from_ptr(ghostty_gtk_host_build_id())
                .to_string_lossy()
                .into_owned();
            GhosttyGtkInfo { version, build_id }
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            GhosttyGtkInfo {
                version: "unavailable".into(),
                build_id: "unavailable".into(),
            }
        }
    }

    pub fn begin_shutdown(&self) {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            ghostty_gtk_host_begin_shutdown(self.raw.as_ptr());
        }
    }

    pub fn surface_count(&self) -> usize {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            ghostty_gtk_host_surface_count(self.raw.as_ptr())
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            0
        }
    }

    pub fn create_surface(
        &self,
        descriptor: &GhosttyGtkSurfaceDescriptor,
    ) -> Result<Widget, GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let cwd = descriptor
                .cwd
                .as_deref()
                .map(|value| CString::new(value).map_err(|_| GhosttyGtkError::InvalidString("cwd")))
                .transpose()?;
            let title = descriptor
                .title
                .as_deref()
                .map(|value| {
                    CString::new(value).map_err(|_| GhosttyGtkError::InvalidString("title"))
                })
                .transpose()?;
            let env_entries = descriptor
                .env
                .iter()
                .map(|(key, value)| {
                    CString::new(format!("{key}={value}"))
                        .map_err(|_| GhosttyGtkError::InvalidString("env"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let env_entry_ptrs = env_entries
                .iter()
                .map(|value| value.as_ptr())
                .collect::<Vec<_>>();

            let options = ghostty_gtk_surface_options_s {
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

            let widget = ghostty_gtk_surface_new(self.raw.as_ptr(), &options);
            if widget.is_null() {
                return Err(GhosttyGtkError::SurfaceInit);
            }

            Ok(from_glib_full(widget.cast()))
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = descriptor;
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn focus_surface(&self, widget: &Widget) -> Result<(), GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let ok = ghostty_gtk_surface_grab_focus(widget.as_ptr().cast());
            if ok == 0 {
                Err(GhosttyGtkError::SurfaceInit)
            } else {
                Ok(())
            }
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = widget;
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn destroy_surface(&self, widget: &Widget) {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            ghostty_gtk_surface_destroy(widget.as_ptr().cast());
        }
    }

    pub fn surface_has_selection(&self, widget: &Widget) -> Result<bool, GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            Ok(ghostty_gtk_surface_has_selection(widget.as_ptr().cast()) != 0)
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = widget;
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn read_surface_text(&self, widget: &Widget) -> Result<String, GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let mut text = ghostty_gtk_text_s::default();
            let ok = ghostty_gtk_surface_read_all_text(widget.as_ptr().cast(), &mut text);
            if ok == 0 {
                return Err(GhosttyGtkError::SurfaceReadText);
            }

            let bytes = if text.text.is_null() || text.text_len == 0 {
                &[]
            } else {
                slice::from_raw_parts(text.text.cast::<u8>(), text.text_len)
            };
            let output = String::from_utf8_lossy(bytes).into_owned();
            ghostty_gtk_surface_free_text(&mut text);
            Ok(output)
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = widget;
            Err(GhosttyGtkError::Unavailable)
        }
    }

    pub fn send_surface_text(&self, widget: &Widget, text: &str) -> Result<(), GhosttyGtkError> {
        #[cfg(ghostty_gtk_bridge)]
        unsafe {
            let text =
                CString::new(text).map_err(|_| GhosttyGtkError::InvalidString("surface_text"))?;
            let ok = ghostty_gtk_surface_send_text(
                widget.as_ptr().cast(),
                text.as_ptr(),
                text.as_bytes().len(),
            );
            if ok == 0 {
                Err(GhosttyGtkError::SurfaceWriteText)
            } else {
                Ok(())
            }
        }

        #[cfg(not(ghostty_gtk_bridge))]
        {
            let _ = (widget, text);
            Err(GhosttyGtkError::Unavailable)
        }
    }
}

#[cfg(ghostty_gtk_bridge)]
impl Drop for GhosttyGtkHost {
    fn drop(&mut self) {
        unsafe {
            ghostty_gtk_host_free(self.raw.as_ptr());
        }
    }
}

#[cfg(ghostty_gtk_bridge)]
#[repr(C)]
struct ghostty_gtk_host_t {
    _private: [u8; 0],
}

#[cfg(ghostty_gtk_bridge)]
#[repr(C)]
struct ghostty_gtk_host_options_s {
    command_argv: *const *const c_char,
    command_argc: usize,
    env_entries: *const *const c_char,
    env_count: usize,
    base_config_path: *const c_char,
    override_config_path: *const c_char,
}

#[cfg(ghostty_gtk_bridge)]
#[repr(C)]
struct ghostty_gtk_surface_options_s {
    working_directory: *const c_char,
    title: *const c_char,
    env_entries: *const *const c_char,
    env_count: usize,
}

#[cfg(ghostty_gtk_bridge)]
#[repr(C)]
#[derive(Default)]
struct ghostty_gtk_text_s {
    text: *const c_char,
    text_len: usize,
}

#[cfg(ghostty_gtk_bridge)]
unsafe extern "C" {
    fn ghostty_gtk_host_new(options: *const ghostty_gtk_host_options_s) -> *mut ghostty_gtk_host_t;
    fn ghostty_gtk_host_free(host: *mut ghostty_gtk_host_t);
    fn ghostty_gtk_host_version() -> *const c_char;
    fn ghostty_gtk_host_build_id() -> *const c_char;
    fn ghostty_gtk_host_begin_shutdown(host: *mut ghostty_gtk_host_t);
    fn ghostty_gtk_host_surface_count(host: *mut ghostty_gtk_host_t) -> usize;
    fn ghostty_gtk_host_tick(host: *mut ghostty_gtk_host_t) -> c_int;
    fn ghostty_gtk_surface_new(
        host: *mut ghostty_gtk_host_t,
        options: *const ghostty_gtk_surface_options_s,
    ) -> *mut std::ffi::c_void;
    fn ghostty_gtk_surface_destroy(widget: *mut std::ffi::c_void);
    fn ghostty_gtk_surface_grab_focus(widget: *mut std::ffi::c_void) -> c_int;
    fn ghostty_gtk_surface_has_selection(widget: *mut std::ffi::c_void) -> c_int;
    fn ghostty_gtk_surface_send_text(
        widget: *mut std::ffi::c_void,
        text: *const c_char,
        len: usize,
    ) -> c_int;
    fn ghostty_gtk_surface_read_all_text(
        widget: *mut std::ffi::c_void,
        result: *mut ghostty_gtk_text_s,
    ) -> c_int;
    fn ghostty_gtk_surface_free_text(result: *mut ghostty_gtk_text_s);
}
