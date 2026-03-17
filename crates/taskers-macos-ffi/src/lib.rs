use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char},
    path::PathBuf,
    ptr,
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use taskers_control::{ControlCommand, default_socket_path};
use taskers_core::{AppState, default_session_path, load_or_bootstrap};
use taskers_domain::{
    AppModel, PaneId, PaneRecord, SurfaceId, SurfaceRecord, WindowId, WindowRecord, Workspace,
    WorkspaceColumnId, WorkspaceColumnRecord, WorkspaceId, WorkspaceWindowId,
    WorkspaceWindowRecord,
};
use taskers_ghostty::BackendChoice;
use taskers_runtime::{ShellLaunchSpec, install_shell_integration};

pub struct TaskersMacosCore {
    app_state: AppState,
    _socket_path: PathBuf,
}

#[derive(Debug, Default, Deserialize)]
struct CoreOptions {
    #[serde(default)]
    session_path: Option<PathBuf>,
    #[serde(default)]
    socket_path: Option<PathBuf>,
    #[serde(default)]
    configured_shell: Option<String>,
    #[serde(default)]
    demo: bool,
    #[serde(default)]
    backend: Option<BackendChoice>,
}

#[derive(Serialize)]
struct MacosSnapshot {
    active_window: WindowId,
    windows: Vec<MacosWindowRecord>,
    workspaces: Vec<MacosWorkspace>,
}

#[derive(Serialize)]
struct MacosWindowRecord {
    id: WindowId,
    workspace_order: Vec<WorkspaceId>,
    active_workspace: WorkspaceId,
}

#[derive(Serialize)]
struct MacosWorkspace {
    id: WorkspaceId,
    label: String,
    columns: Vec<MacosWorkspaceColumn>,
    windows: Vec<MacosWorkspaceWindow>,
    active_window: WorkspaceWindowId,
    panes: Vec<MacosPane>,
    active_pane: PaneId,
}

#[derive(Serialize)]
struct MacosWorkspaceColumn {
    id: WorkspaceColumnId,
    width: i32,
    window_order: Vec<WorkspaceWindowId>,
    active_window: WorkspaceWindowId,
}

#[derive(Serialize)]
struct MacosWorkspaceWindow {
    id: WorkspaceWindowId,
    height: i32,
    layout: taskers_domain::LayoutNode,
    active_pane: PaneId,
}

#[derive(Serialize)]
struct MacosPane {
    id: PaneId,
    surfaces: Vec<MacosSurface>,
    active_surface: SurfaceId,
}

#[derive(Serialize)]
struct MacosSurface {
    id: SurfaceId,
    metadata: MacosSurfaceMetadata,
}

#[derive(Serialize)]
struct MacosSurfaceMetadata {
    title: Option<String>,
    cwd: Option<String>,
}

impl From<AppModel> for MacosSnapshot {
    fn from(value: AppModel) -> Self {
        Self {
            active_window: value.active_window,
            windows: value.windows.into_values().map(MacosWindowRecord::from).collect(),
            workspaces: value.workspaces.into_values().map(MacosWorkspace::from).collect(),
        }
    }
}

impl From<WindowRecord> for MacosWindowRecord {
    fn from(value: WindowRecord) -> Self {
        Self {
            id: value.id,
            workspace_order: value.workspace_order,
            active_workspace: value.active_workspace,
        }
    }
}

impl From<Workspace> for MacosWorkspace {
    fn from(value: Workspace) -> Self {
        Self {
            id: value.id,
            label: value.label,
            columns: value
                .columns
                .into_values()
                .map(MacosWorkspaceColumn::from)
                .collect(),
            windows: value
                .windows
                .into_values()
                .map(MacosWorkspaceWindow::from)
                .collect(),
            active_window: value.active_window,
            panes: value.panes.into_values().map(MacosPane::from).collect(),
            active_pane: value.active_pane,
        }
    }
}

impl From<WorkspaceColumnRecord> for MacosWorkspaceColumn {
    fn from(value: WorkspaceColumnRecord) -> Self {
        Self {
            id: value.id,
            width: value.width,
            window_order: value.window_order,
            active_window: value.active_window,
        }
    }
}

impl From<WorkspaceWindowRecord> for MacosWorkspaceWindow {
    fn from(value: WorkspaceWindowRecord) -> Self {
        Self {
            id: value.id,
            height: value.height,
            layout: value.layout,
            active_pane: value.active_pane,
        }
    }
}

impl From<PaneRecord> for MacosPane {
    fn from(value: PaneRecord) -> Self {
        Self {
            id: value.id,
            surfaces: value.surfaces.into_values().map(MacosSurface::from).collect(),
            active_surface: value.active_surface,
        }
    }
}

impl From<SurfaceRecord> for MacosSurface {
    fn from(value: SurfaceRecord) -> Self {
        Self {
            id: value.id,
            metadata: MacosSurfaceMetadata {
                title: value.metadata.title,
                cwd: value.metadata.cwd,
            },
        }
    }
}

thread_local! {
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_error(message: impl Into<String>) {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = Some(message.into());
    });
}

fn clear_last_error() {
    LAST_ERROR.with(|slot| {
        *slot.borrow_mut() = None;
    });
}

impl TaskersMacosCore {
    fn new_with_options(options: CoreOptions) -> Result<Self, String> {
        let session_path = options.session_path.unwrap_or_else(default_session_path);
        let socket_path = options.socket_path.unwrap_or_else(default_socket_path);
        let model = load_or_bootstrap(&session_path, options.demo)
            .map_err(|error| format!("failed to initialize session state: {error}"))?;

        let (mut shell_launch, shell_integration_error) =
            match install_shell_integration(options.configured_shell.as_deref()) {
                Ok(integration) => (integration.launch_spec(), None),
                Err(error) => (
                    ShellLaunchSpec::fallback(),
                    Some(format!("shell integration unavailable: {error}")),
                ),
            };
        shell_launch
            .env
            .insert("TASKERS_SOCKET".into(), socket_path.display().to_string());

        let backend_choice = options.backend.unwrap_or(BackendChoice::GhosttyEmbedded);
        let app_state = AppState::new(model, session_path, backend_choice, shell_launch)
            .map_err(|error| format!("failed to create shared app state: {error}"))?;

        if let Some(error) = shell_integration_error {
            set_last_error(error);
        } else {
            clear_last_error();
        }

        Ok(Self {
            app_state,
            _socket_path: socket_path,
        })
    }

    fn new(
        session_path: Option<PathBuf>,
        socket_path: Option<PathBuf>,
        configured_shell: Option<&str>,
        demo: bool,
    ) -> Result<Self, String> {
        Self::new_with_options(CoreOptions {
            session_path,
            socket_path,
            configured_shell: configured_shell.map(str::to_string),
            demo,
            backend: Some(BackendChoice::GhosttyEmbedded),
        })
    }

    fn snapshot_json(&self) -> Result<String, String> {
        serde_json::to_string(&MacosSnapshot::from(self.app_state.snapshot_model()))
            .map_err(|error| format!("failed to serialize snapshot: {error}"))
    }

    fn dispatch_json(&mut self, command_json: &str) -> Result<String, String> {
        let command = serde_json::from_str::<ControlCommand>(command_json)
            .map_err(|error| format!("failed to decode command JSON: {error}"))?;
        let response = self
            .app_state
            .dispatch(command)
            .map_err(|error| format!("command failed: {error}"))?;
        serde_json::to_string(&response)
            .map_err(|error| format!("failed to serialize response: {error}"))
    }

    fn surface_descriptor_json(&self, workspace_id: &str, pane_id: &str) -> Result<String, String> {
        let workspace_id = WorkspaceId::from_str(workspace_id)
            .map_err(|error| format!("invalid workspace id: {error}"))?;
        let pane_id =
            PaneId::from_str(pane_id).map_err(|error| format!("invalid pane id: {error}"))?;
        let descriptor = self
            .app_state
            .surface_descriptor_for_pane(workspace_id, pane_id)
            .map_err(|error| format!("failed to build surface descriptor: {error}"))?;
        serde_json::to_string(&descriptor)
            .map_err(|error| format!("failed to serialize surface descriptor: {error}"))
    }
}

fn optional_path_from_ptr(value: *const c_char) -> Result<Option<PathBuf>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|error| format!("argument contained invalid UTF-8: {error}"))?
        .trim()
        .to_string();
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(value)))
    }
}

fn optional_string_from_ptr(value: *const c_char) -> Result<Option<String>, String> {
    if value.is_null() {
        return Ok(None);
    }
    let value = unsafe { CStr::from_ptr(value) }
        .to_str()
        .map_err(|error| format!("argument contained invalid UTF-8: {error}"))?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed.to_string()))
    }
}

fn options_from_json_ptr(value: *const c_char) -> Result<CoreOptions, String> {
    match optional_string_from_ptr(value)? {
        Some(value) => serde_json::from_str(&value)
            .map_err(|error| format!("failed to decode options JSON: {error}")),
        None => Ok(CoreOptions::default()),
    }
}

fn string_into_ptr(value: String) -> *mut c_char {
    match CString::new(value) {
        Ok(value) => value.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_new_with_options_json(
    options_json: *const c_char,
) -> *mut TaskersMacosCore {
    let options = match options_from_json_ptr(options_json) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };

    match TaskersMacosCore::new_with_options(options) {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

fn with_core_mut<R>(
    core: *mut TaskersMacosCore,
    f: impl FnOnce(&mut TaskersMacosCore) -> Result<R, String>,
) -> Option<R> {
    if core.is_null() {
        set_last_error("taskers macOS core handle was null");
        return None;
    }

    let core = unsafe { &mut *core };
    match f(core) {
        Ok(value) => {
            clear_last_error();
            Some(value)
        }
        Err(error) => {
            set_last_error(error);
            None
        }
    }
}

fn with_core<R>(
    core: *const TaskersMacosCore,
    f: impl FnOnce(&TaskersMacosCore) -> Result<R, String>,
) -> Option<R> {
    if core.is_null() {
        set_last_error("taskers macOS core handle was null");
        return None;
    }

    let core = unsafe { &*core };
    match f(core) {
        Ok(value) => {
            clear_last_error();
            Some(value)
        }
        Err(error) => {
            set_last_error(error);
            None
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_new(
    session_path: *const c_char,
    socket_path: *const c_char,
    configured_shell: *const c_char,
    demo: bool,
) -> *mut TaskersMacosCore {
    let session_path = match optional_path_from_ptr(session_path) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };
    let socket_path = match optional_path_from_ptr(socket_path) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };
    let configured_shell = match optional_string_from_ptr(configured_shell) {
        Ok(value) => value,
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };

    match TaskersMacosCore::new(session_path, socket_path, configured_shell.as_deref(), demo) {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(error) => {
            set_last_error(error);
            ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_free(core: *mut TaskersMacosCore) {
    if core.is_null() {
        return;
    }

    unsafe {
        drop(Box::from_raw(core));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_snapshot_json(core: *const TaskersMacosCore) -> *mut c_char {
    with_core(core, TaskersMacosCore::snapshot_json)
        .map(string_into_ptr)
        .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_dispatch_json(
    core: *mut TaskersMacosCore,
    command_json: *const c_char,
) -> *mut c_char {
    let command_json = match optional_string_from_ptr(command_json) {
        Ok(Some(value)) => value,
        Ok(None) => {
            set_last_error("command JSON must not be empty");
            return ptr::null_mut();
        }
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };

    with_core_mut(core, |core| core.dispatch_json(&command_json))
        .map(string_into_ptr)
        .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_surface_descriptor_json(
    core: *const TaskersMacosCore,
    workspace_id: *const c_char,
    pane_id: *const c_char,
) -> *mut c_char {
    let workspace_id = match optional_string_from_ptr(workspace_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            set_last_error("workspace id must not be empty");
            return ptr::null_mut();
        }
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };
    let pane_id = match optional_string_from_ptr(pane_id) {
        Ok(Some(value)) => value,
        Ok(None) => {
            set_last_error("pane id must not be empty");
            return ptr::null_mut();
        }
        Err(error) => {
            set_last_error(error);
            return ptr::null_mut();
        }
    };

    with_core(core, |core| {
        core.surface_descriptor_json(&workspace_id, &pane_id)
    })
    .map(string_into_ptr)
    .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_core_revision(core: *const TaskersMacosCore) -> u64 {
    with_core(core, |core| Ok(core.app_state.revision())).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_last_error_message() -> *mut c_char {
    LAST_ERROR
        .with(|slot| slot.borrow().clone())
        .map(string_into_ptr)
        .unwrap_or(ptr::null_mut())
}

#[unsafe(no_mangle)]
pub extern "C" fn taskers_macos_string_free(value: *mut c_char) {
    if value.is_null() {
        return;
    }

    unsafe {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tempfile::tempdir;

    use super::{CoreOptions, TaskersMacosCore};

    #[test]
    fn core_roundtrips_snapshot_and_dispatch_json() {
        let temp = tempdir().expect("tempdir");
        let session_path = temp.path().join("session.json");
        let socket_path = temp.path().join("taskers.sock");
        let mut core = TaskersMacosCore::new(
            Some(session_path),
            Some(socket_path),
            Some("/bin/sh"),
            false,
        )
        .expect("core");

        let snapshot = core.snapshot_json().expect("snapshot");
        let snapshot: Value = serde_json::from_str(&snapshot).expect("snapshot json");
        let workspaces = snapshot
            .get("workspaces")
            .and_then(Value::as_array)
            .expect("workspaces array");
        assert!(!workspaces.is_empty());
        assert!(
            workspaces[0]
                .get("columns")
                .and_then(Value::as_array)
                .is_some()
        );

        let response = core
            .dispatch_json(r#"{"command":"create_workspace","label":"Docs"}"#)
            .expect("dispatch");
        let response: Value = serde_json::from_str(&response).expect("response json");
        assert_eq!(
            response.get("status").and_then(Value::as_str),
            Some("workspace_created")
        );
        assert_eq!(core.app_state.revision(), 1);
    }

    #[test]
    fn surface_descriptor_json_includes_shell_command() {
        let temp = tempdir().expect("tempdir");
        let session_path = temp.path().join("session.json");
        let socket_path = temp.path().join("taskers.sock");
        let core = TaskersMacosCore::new(
            Some(session_path),
            Some(socket_path),
            Some("/bin/sh"),
            false,
        )
        .expect("core");

        let model = core.app_state.snapshot_model();
        let workspace_id = model.active_workspace_id().expect("workspace").to_string();
        let pane_id = model
            .active_workspace()
            .expect("workspace")
            .active_pane
            .to_string();

        let descriptor = core
            .surface_descriptor_json(&workspace_id, &pane_id)
            .expect("descriptor");
        let descriptor: Value = serde_json::from_str(&descriptor).expect("descriptor json");
        let command_argv = descriptor
            .get("command_argv")
            .and_then(Value::as_array)
            .expect("command argv array");
        assert!(!command_argv.is_empty());
        assert_eq!(
            descriptor
                .get("env")
                .and_then(Value::as_object)
                .and_then(|env| env.get("TASKERS_SOCKET"))
                .and_then(Value::as_str),
            Some(
                temp.path()
                    .join("taskers.sock")
                    .to_str()
                    .expect("utf-8 socket path")
            )
        );
    }

    #[test]
    fn options_json_supports_explicit_mock_backend() {
        let temp = tempdir().expect("tempdir");
        let options = serde_json::from_value::<CoreOptions>(json!({
            "session_path": temp.path().join("session.json"),
            "socket_path": temp.path().join("taskers.sock"),
            "configured_shell": "/bin/sh",
            "backend": "mock"
        }))
        .expect("options");
        let core = TaskersMacosCore::new_with_options(options).expect("core");

        assert_eq!(core.app_state.revision(), 0);
        assert!(
            core.app_state
                .snapshot_model()
                .active_workspace_id()
                .is_some()
        );
    }
}
