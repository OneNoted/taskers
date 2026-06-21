use std::{
    env,
    ffi::CString,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use tar::Archive;
use thiserror::Error;
use xz2::read::XzDecoder;

const LEGACY_BRIDGE_LIBRARY_NAME: &str = "libtaskers_ghostty_bridge.so";
const GTK_BRIDGE_LIBRARY_NAME: &str = "libghostty_gtk.so";
const GTK_BRIDGE_PATH_ENV: &str = "GHOSTTY_GTK_BRIDGE_PATH";
const GTK_RUNTIME_DIR_ENV: &str = "GHOSTTY_GTK_RUNTIME_DIR";
const GTK_BUNDLE_PATH_ENV: &str = "GHOSTTY_GTK_RUNTIME_BUNDLE_PATH";
const GTK_BUNDLE_URL_ENV: &str = "GHOSTTY_GTK_RUNTIME_URL";
const GTK_DISABLE_BOOTSTRAP_ENV: &str = "GHOSTTY_GTK_DISABLE_RUNTIME_BOOTSTRAP";
const RUNTIME_VERSION_FILE: &str = ".taskers-runtime-version";
const TERMINFO_GHOSTTY_PATH: &str = "g/ghostty";
const TERMINFO_XTERM_GHOSTTY_PATH: &str = "x/xterm-ghostty";
const LEGACY_BUNDLE_PATH_ENV: &str = "TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH";
const LEGACY_BUNDLE_URL_ENV: &str = "TASKERS_GHOSTTY_RUNTIME_URL";
const LEGACY_DISABLE_BOOTSTRAP_ENV: &str = "TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBootstrap {
    pub runtime_dir: PathBuf,
}

#[derive(Debug, Error)]
pub enum RuntimeBootstrapError {
    #[error("failed to create Ghostty runtime directory at {path}: {message}")]
    CreateDir { path: PathBuf, message: String },
    #[error("failed to remove existing Ghostty runtime path at {path}: {message}")]
    RemovePath { path: PathBuf, message: String },
    #[error("failed to rename Ghostty runtime path from {from} to {to}: {message}")]
    RenamePath {
        from: PathBuf,
        to: PathBuf,
        message: String,
    },
    #[error("failed to open Ghostty runtime bundle at {path}: {message}")]
    OpenBundle { path: PathBuf, message: String },
    #[error("failed to download Ghostty runtime bundle from {url}: {message}")]
    DownloadBundle { url: String, message: String },
    #[error("failed to unpack Ghostty runtime bundle into {path}: {message}")]
    UnpackBundle { path: PathBuf, message: String },
    #[error("failed to copy Ghostty runtime path from {from} to {to}: {message}")]
    CopyPath {
        from: PathBuf,
        to: PathBuf,
        message: String,
    },
    #[error("Ghostty runtime bundle missing required file {path}")]
    MissingBundlePath { path: &'static str },
    #[error("failed to write Ghostty runtime version marker at {path}: {message}")]
    WriteVersion { path: PathBuf, message: String },
}

pub fn ensure_runtime_installed() -> Result<Option<RuntimeBootstrap>, RuntimeBootstrapError> {
    if env::var_os(GTK_DISABLE_BOOTSTRAP_ENV).is_some()
        || env::var_os(LEGACY_DISABLE_BOOTSTRAP_ENV).is_some()
    {
        return Ok(None);
    }

    let current_exe = current_exe_path();
    let bundle_override = env::var_os(GTK_BUNDLE_PATH_ENV).is_some()
        || env::var_os(LEGACY_BUNDLE_PATH_ENV).is_some()
        || env::var_os(GTK_BUNDLE_URL_ENV).is_some()
        || env::var_os(LEGACY_BUNDLE_URL_ENV).is_some();
    let build_runtime = build_runtime_layout();
    if !bundle_override
        && build_runtime.is_some()
        && use_build_runtime_directly_for(current_exe.as_deref())
    {
        return Ok(None);
    }

    let Some(runtime_dir) = installed_runtime_dir() else {
        return Ok(None);
    };
    normalize_gtk_bridge_layout(&runtime_dir)?;
    if !bundle_override && installed_runtime_is_current(&runtime_dir) {
        return Ok(None);
    }

    let taskers_root = runtime_dir
        .parent()
        .expect("ghostty runtime dir should have a parent")
        .to_path_buf();
    fs::create_dir_all(&taskers_root).map_err(|error| RuntimeBootstrapError::CreateDir {
        path: taskers_root.clone(),
        message: error.to_string(),
    })?;

    let staging_root = taskers_root.join(format!(".ghostty-runtime-stage-{}", std::process::id()));
    remove_path_if_exists(&staging_root)?;
    fs::create_dir_all(&staging_root).map_err(|error| RuntimeBootstrapError::CreateDir {
        path: staging_root.clone(),
        message: error.to_string(),
    })?;

    let install_result = if let Some(bundle_path) = env::var_os(GTK_BUNDLE_PATH_ENV)
        .or_else(|| env::var_os(LEGACY_BUNDLE_PATH_ENV))
        .map(PathBuf::from)
    {
        let file =
            fs::File::open(&bundle_path).map_err(|error| RuntimeBootstrapError::OpenBundle {
                path: bundle_path.clone(),
                message: error.to_string(),
            })?;
        unpack_bundle(file, &staging_root)
    } else if let Some(build_runtime) = build_runtime.as_ref() {
        stage_build_runtime_layout(build_runtime, &staging_root)
    } else {
        let url = env::var(GTK_BUNDLE_URL_ENV)
            .or_else(|_| env::var(LEGACY_BUNDLE_URL_ENV))
            .unwrap_or_else(|_| default_runtime_bundle_url());
        let response =
            ureq::get(&url)
                .call()
                .map_err(|error| RuntimeBootstrapError::DownloadBundle {
                    url: url.clone(),
                    message: error.to_string(),
                })?;
        unpack_bundle(response.into_reader(), &staging_root).map_err(|error| match error {
            RuntimeBootstrapError::UnpackBundle { .. }
            | RuntimeBootstrapError::CopyPath { .. }
            | RuntimeBootstrapError::MissingBundlePath { .. }
            | RuntimeBootstrapError::WriteVersion { .. }
            | RuntimeBootstrapError::CreateDir { .. }
            | RuntimeBootstrapError::RemovePath { .. }
            | RuntimeBootstrapError::RenamePath { .. }
            | RuntimeBootstrapError::OpenBundle { .. }
            | RuntimeBootstrapError::DownloadBundle { .. } => error,
        })
    };
    if let Err(error) = install_result {
        let _ = remove_path_if_exists(&staging_root);
        return Err(error);
    }

    let ghostty_stage = staging_root.join("ghostty");
    let terminfo_stage = staging_root.join("terminfo");
    normalize_gtk_bridge_layout(&ghostty_stage)?;
    validate_bundle(&ghostty_stage, &terminfo_stage)?;

    let version_marker_path = ghostty_stage.join(RUNTIME_VERSION_FILE);
    fs::write(&version_marker_path, env!("CARGO_PKG_VERSION")).map_err(|error| {
        RuntimeBootstrapError::WriteVersion {
            path: version_marker_path.clone(),
            message: error.to_string(),
        }
    })?;

    let terminfo_dir = taskers_root.join("terminfo");
    replace_directory(&ghostty_stage, &runtime_dir)?;
    replace_directory(&terminfo_stage, &terminfo_dir)?;
    let _ = remove_path_if_exists(&staging_root);

    Ok(Some(RuntimeBootstrap { runtime_dir }))
}

pub fn configure_runtime_environment() {
    if env::var_os("GHOSTTY_RESOURCES_DIR").is_some() {
        return;
    }

    let current_exe = current_exe_path();
    if let Some(path) = runtime_resources_dir_for(current_exe.as_deref()) {
        set_runtime_environment_vars(&path);
    }
}

pub fn runtime_resources_dir() -> Option<PathBuf> {
    runtime_resources_dir_for(current_exe_path().as_deref())
}

fn runtime_resources_dir_for(current_exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = env::var_os("GHOSTTY_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = explicit_runtime_dir().filter(|path| path.exists()) {
        return Some(path);
    }

    if use_build_runtime_directly_for(current_exe)
        && let Some(path) = build_runtime_layout().map(|layout| layout.resources_dir)
    {
        return Some(path);
    }

    normalized_default_installed_runtime_dir()
}

pub fn runtime_gtk_bridge_path() -> Option<PathBuf> {
    runtime_gtk_bridge_path_for(current_exe_path().as_deref())
}

fn runtime_gtk_bridge_path_for(current_exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = env::var_os(GTK_BRIDGE_PATH_ENV)
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = env::var_os("TASKERS_GHOSTTY_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = explicit_runtime_dir().and_then(|root| {
        gtk_bridge_library_paths_in_dir(&root.join("lib")).find(|path| path.exists())
    }) {
        return Some(path);
    }

    if use_build_runtime_directly_for(current_exe)
        && let Some(path) = build_runtime_layout().map(|layout| layout.gtk_bridge_path)
    {
        return Some(path);
    }

    normalized_default_installed_runtime_dir().and_then(|root| {
        gtk_bridge_library_paths_in_dir(&root.join("lib")).find(|path| path.exists())
    })
}

pub fn runtime_terminfo_dir() -> Option<PathBuf> {
    runtime_terminfo_dir_for(current_exe_path().as_deref())
}

fn runtime_terminfo_dir_for(current_exe: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = env::var_os("TERMINFO")
        .map(PathBuf::from)
        .filter(|path| terminfo_dir_is_usable(path))
    {
        return Some(path);
    }

    if let Some(path) = explicit_runtime_dir()
        .and_then(|root| sibling_terminfo_dir(&root))
        .filter(|path| terminfo_dir_is_usable(path))
    {
        return Some(path);
    }

    if use_build_runtime_directly_for(current_exe)
        && let Some(path) = build_runtime_layout().map(|layout| layout.terminfo_dir)
    {
        return Some(path);
    }

    default_installed_runtime_dir()
        .and_then(|root| sibling_terminfo_dir(&root))
        .filter(|path| terminfo_dir_is_usable(path))
}

fn unpack_bundle<R: Read>(reader: R, staging_root: &Path) -> Result<(), RuntimeBootstrapError> {
    let decoder = XzDecoder::new(reader);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(staging_root)
        .map_err(|error| RuntimeBootstrapError::UnpackBundle {
            path: staging_root.to_path_buf(),
            message: error.to_string(),
        })
}

fn validate_bundle(ghostty_dir: &Path, terminfo_dir: &Path) -> Result<(), RuntimeBootstrapError> {
    if !ghostty_dir
        .join("lib")
        .join(GTK_BRIDGE_LIBRARY_NAME)
        .exists()
    {
        return Err(RuntimeBootstrapError::MissingBundlePath {
            path: "ghostty/lib/libghostty_gtk.so",
        });
    }
    if !terminfo_dir.join(TERMINFO_GHOSTTY_PATH).exists()
        && !terminfo_dir.join(TERMINFO_XTERM_GHOSTTY_PATH).exists()
    {
        return Err(RuntimeBootstrapError::MissingBundlePath {
            path: "terminfo/g/ghostty or terminfo/x/xterm-ghostty",
        });
    }
    Ok(())
}

fn normalize_gtk_bridge_layout(ghostty_dir: &Path) -> Result<(), RuntimeBootstrapError> {
    let ghostty_lib_dir = ghostty_dir.join("lib");
    let generic_bridge_path = ghostty_lib_dir.join(GTK_BRIDGE_LIBRARY_NAME);
    if generic_bridge_path.exists() {
        return Ok(());
    }

    let legacy_bridge_path = ghostty_lib_dir.join(LEGACY_BRIDGE_LIBRARY_NAME);
    if !legacy_bridge_path.exists() {
        return Ok(());
    }

    fs::copy(&legacy_bridge_path, &generic_bridge_path).map_err(|error| {
        RuntimeBootstrapError::CopyPath {
            from: legacy_bridge_path,
            to: generic_bridge_path,
            message: error.to_string(),
        }
    })?;
    Ok(())
}

fn installed_runtime_is_current(runtime_dir: &Path) -> bool {
    let generic_bridge_path = runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME);
    if !generic_bridge_path.exists()
        || !gtk_bridge_library_has_required_symbols(&generic_bridge_path)
    {
        return false;
    }

    let Some(taskers_root) = runtime_dir.parent() else {
        return false;
    };
    let terminfo_dir = taskers_root.join("terminfo");
    if !terminfo_dir.join(TERMINFO_GHOSTTY_PATH).exists()
        && !terminfo_dir.join(TERMINFO_XTERM_GHOSTTY_PATH).exists()
    {
        return false;
    }

    match fs::read_to_string(runtime_dir.join(RUNTIME_VERSION_FILE)) {
        Ok(version) => version.trim() == env!("CARGO_PKG_VERSION"),
        Err(_) => true,
    }
}

fn gtk_bridge_library_has_required_symbols(path: &Path) -> bool {
    let Ok(c_path) = CString::new(path.to_string_lossy().as_bytes()) else {
        return false;
    };

    unsafe {
        let handle = libc::dlopen(c_path.as_ptr(), libc::RTLD_LOCAL | libc::RTLD_LAZY);
        if handle.is_null() {
            return false;
        }
        let host_new = libc::dlsym(handle, c"ghostty_gtk_host_new".as_ptr().cast());
        let surface_new = libc::dlsym(handle, c"ghostty_gtk_surface_new".as_ptr().cast());
        let _ = libc::dlclose(handle);
        !host_new.is_null() && !surface_new.is_null()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildRuntimeLayout {
    gtk_bridge_path: PathBuf,
    resources_dir: PathBuf,
    terminfo_dir: PathBuf,
}

fn build_runtime_layout() -> Option<BuildRuntimeLayout> {
    Some(BuildRuntimeLayout {
        gtk_bridge_path: build_runtime_gtk_bridge_path()?,
        resources_dir: build_runtime_resources_dir()?,
        terminfo_dir: build_runtime_terminfo_dir()?,
    })
}

fn use_build_runtime_directly_for(current_exe: Option<&Path>) -> bool {
    let Some(current_exe) = current_exe else {
        return false;
    };
    current_exe.starts_with(repo_target_dir())
}

fn current_exe_path() -> Option<PathBuf> {
    env::current_exe().ok()
}

fn repo_target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("taskers-ghostty should live under the workspace crates directory")
        .join("target")
}

fn build_runtime_gtk_bridge_path() -> Option<PathBuf> {
    option_env!("GHOSTTY_GTK_BUILD_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn build_runtime_resources_dir() -> Option<PathBuf> {
    option_env!("GHOSTTY_GTK_BUILD_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn build_runtime_terminfo_dir() -> Option<PathBuf> {
    option_env!("GHOSTTY_GTK_BUILD_TERMINFO_DIR")
        .map(PathBuf::from)
        .filter(|path| terminfo_dir_is_usable(path))
}

fn stage_build_runtime_layout(
    layout: &BuildRuntimeLayout,
    staging_root: &Path,
) -> Result<(), RuntimeBootstrapError> {
    let ghostty_stage = staging_root.join("ghostty");
    copy_dir_all(&layout.resources_dir, &ghostty_stage)?;

    let generic_bridge_destination = ghostty_stage.join("lib").join(GTK_BRIDGE_LIBRARY_NAME);
    if let Some(parent) = generic_bridge_destination.parent() {
        fs::create_dir_all(parent).map_err(|error| RuntimeBootstrapError::CreateDir {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    fs::copy(&layout.gtk_bridge_path, &generic_bridge_destination).map_err(|error| {
        RuntimeBootstrapError::CopyPath {
            from: layout.gtk_bridge_path.clone(),
            to: generic_bridge_destination.clone(),
            message: error.to_string(),
        }
    })?;

    let legacy_bridge_destination = ghostty_stage.join("lib").join(LEGACY_BRIDGE_LIBRARY_NAME);
    if legacy_bridge_destination != generic_bridge_destination {
        fs::copy(&generic_bridge_destination, &legacy_bridge_destination).map_err(|error| {
            RuntimeBootstrapError::CopyPath {
                from: generic_bridge_destination.clone(),
                to: legacy_bridge_destination.clone(),
                message: error.to_string(),
            }
        })?;
    }

    copy_dir_all(&layout.terminfo_dir, &staging_root.join("terminfo"))
}

fn gtk_bridge_library_paths_in_dir(lib_dir: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    [GTK_BRIDGE_LIBRARY_NAME, LEGACY_BRIDGE_LIBRARY_NAME]
        .into_iter()
        .map(|name| lib_dir.join(name))
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<(), RuntimeBootstrapError> {
    fs::create_dir_all(destination).map_err(|error| RuntimeBootstrapError::CreateDir {
        path: destination.to_path_buf(),
        message: error.to_string(),
    })?;

    for entry in fs::read_dir(source).map_err(|error| RuntimeBootstrapError::CopyPath {
        from: source.to_path_buf(),
        to: destination.to_path_buf(),
        message: error.to_string(),
    })? {
        let entry = entry.map_err(|error| RuntimeBootstrapError::CopyPath {
            from: source.to_path_buf(),
            to: destination.to_path_buf(),
            message: error.to_string(),
        })?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| RuntimeBootstrapError::CopyPath {
                from: source_path.clone(),
                to: destination_path.clone(),
                message: error.to_string(),
            })?;

        if file_type.is_dir() {
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                RuntimeBootstrapError::CopyPath {
                    from: source_path.clone(),
                    to: destination_path.clone(),
                    message: error.to_string(),
                }
            })?;
        }
    }

    Ok(())
}

fn set_runtime_environment_vars(path: &Path) {
    unsafe {
        env::set_var("GHOSTTY_RESOURCES_DIR", path);
        env::set_var(GTK_RUNTIME_DIR_ENV, path);
    }
}

fn installed_runtime_dir() -> Option<PathBuf> {
    explicit_runtime_dir().or_else(default_installed_runtime_dir)
}

fn explicit_runtime_dir() -> Option<PathBuf> {
    env::var_os(GTK_RUNTIME_DIR_ENV)
        .or_else(|| env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR"))
        .map(PathBuf::from)
}

fn default_installed_runtime_dir() -> Option<PathBuf> {
    Some(taskers_paths::default_ghostty_runtime_dir())
}

fn normalized_default_installed_runtime_dir() -> Option<PathBuf> {
    let runtime_dir = default_installed_runtime_dir()?;
    if !runtime_dir.exists() {
        return None;
    }
    let _ = normalize_gtk_bridge_layout(&runtime_dir);
    Some(runtime_dir)
}

fn default_runtime_bundle_url() -> String {
    format!(
        "https://github.com/OneNoted/taskers/releases/download/v{version}/taskers-ghostty-runtime-v{version}-{target}.tar.xz",
        version = env!("CARGO_PKG_VERSION"),
        target = option_env!("TASKERS_BUILD_TARGET").unwrap_or("x86_64-unknown-linux-gnu"),
    )
}

fn sibling_terminfo_dir(runtime_dir: &Path) -> Option<PathBuf> {
    runtime_dir.parent().map(|root| root.join("terminfo"))
}

fn terminfo_dir_is_usable(path: &Path) -> bool {
    path.join(TERMINFO_GHOSTTY_PATH).exists() || path.join(TERMINFO_XTERM_GHOSTTY_PATH).exists()
}

fn replace_directory(source: &Path, destination: &Path) -> Result<(), RuntimeBootstrapError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| RuntimeBootstrapError::CreateDir {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
    }
    remove_path_if_exists(destination)?;
    fs::rename(source, destination).map_err(|error| RuntimeBootstrapError::RenamePath {
        from: source.to_path_buf(),
        to: destination.to_path_buf(),
        message: error.to_string(),
    })
}

fn remove_path_if_exists(path: &Path) -> Result<(), RuntimeBootstrapError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| RuntimeBootstrapError::RemovePath {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
    } else {
        fs::remove_file(path).map_err(|error| RuntimeBootstrapError::RemovePath {
            path: path.to_path_buf(),
            message: error.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BuildRuntimeLayout, GTK_BRIDGE_LIBRARY_NAME, GTK_BRIDGE_PATH_ENV, GTK_BUNDLE_PATH_ENV,
        GTK_BUNDLE_URL_ENV, GTK_DISABLE_BOOTSTRAP_ENV, GTK_RUNTIME_DIR_ENV,
        LEGACY_BRIDGE_LIBRARY_NAME, RUNTIME_VERSION_FILE, RuntimeBootstrap,
        ensure_runtime_installed, gtk_bridge_library_has_required_symbols,
        installed_runtime_is_current, normalize_gtk_bridge_layout, runtime_gtk_bridge_path,
        runtime_gtk_bridge_path_for, runtime_resources_dir, runtime_resources_dir_for,
        runtime_terminfo_dir, runtime_terminfo_dir_for, stage_build_runtime_layout, unpack_bundle,
        use_build_runtime_directly_for,
    };
    use std::{env, fs, io::Cursor, path::Path, sync::Mutex};
    use tar::Builder;
    use tempfile::tempdir;
    use xz2::write::XzEncoder;

    static RUNTIME_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn unpack_bundle_does_not_escape_staging_root() {
        let temp = tempdir().expect("tempdir");
        let staging_root = temp.path().join("stage");
        fs::create_dir_all(&staging_root).expect("staging root");
        let escaped_path = temp.path().join("escaped.txt");

        let tar = raw_tar_with_file("../escaped.txt", b"escaped");
        let mut archive = Vec::new();
        {
            let encoder = XzEncoder::new(&mut archive, 9);
            use std::io::Write as _;
            let mut encoder = encoder;
            encoder.write_all(&tar).expect("write xz tar");
            encoder.finish().expect("finish xz");
        }

        let _ = unpack_bundle(Cursor::new(archive), &staging_root);

        assert!(!escaped_path.exists());
    }

    #[test]
    fn local_bundle_bootstrap_installs_runtime_layout() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let bundle_path = temp.path().join("ghostty-runtime.tar.xz");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");

        let bundle_source = temp.path().join("bundle-source");
        fs::create_dir_all(bundle_source.join("ghostty/lib")).expect("ghostty lib dir");
        fs::create_dir_all(bundle_source.join("ghostty/shell-integration/bash"))
            .expect("shell integration dir");
        fs::create_dir_all(bundle_source.join("terminfo/x")).expect("terminfo dir");
        fs::write(
            bundle_source
                .join("ghostty")
                .join("lib")
                .join(GTK_BRIDGE_LIBRARY_NAME),
            b"fake bridge",
        )
        .expect("write fake bridge");
        fs::write(
            bundle_source
                .join("ghostty")
                .join("shell-integration")
                .join("bash")
                .join("ghostty.bash"),
            b"echo ghostty",
        )
        .expect("write fake shell integration");
        fs::write(
            bundle_source
                .join("terminfo")
                .join("x")
                .join("xterm-ghostty"),
            b"fake terminfo",
        )
        .expect("write fake terminfo");
        write_bundle(&bundle_source, &bundle_path);

        let _guard = EnvGuard::set([
            (GTK_BUNDLE_PATH_ENV, Some(bundle_path.as_os_str())),
            (GTK_BUNDLE_URL_ENV, None),
            (GTK_DISABLE_BOOTSTRAP_ENV, None),
            (GTK_RUNTIME_DIR_ENV, Some(runtime_dir.as_os_str())),
            (GTK_BRIDGE_PATH_ENV, None),
            ("TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH", None),
            ("TASKERS_GHOSTTY_RUNTIME_URL", None),
            ("TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP", None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", None),
            ("TASKERS_GHOSTTY_BRIDGE_PATH", None),
            ("GHOSTTY_RESOURCES_DIR", None),
            ("XDG_DATA_HOME", None),
        ]);

        let result = ensure_runtime_installed().expect("runtime install");
        assert_eq!(
            result,
            Some(RuntimeBootstrap {
                runtime_dir: runtime_dir.clone(),
            })
        );
        assert!(
            runtime_dir
                .join("lib")
                .join(GTK_BRIDGE_LIBRARY_NAME)
                .exists()
        );
        assert!(
            runtime_dir
                .join("shell-integration")
                .join("bash")
                .join("ghostty.bash")
                .exists()
        );
        assert!(terminfo_dir.join("x").join("xterm-ghostty").exists());
        assert_eq!(
            fs::read_to_string(runtime_dir.join(RUNTIME_VERSION_FILE))
                .expect("runtime version marker")
                .trim(),
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            runtime_gtk_bridge_path(),
            Some(runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME))
        );
        assert_eq!(runtime_resources_dir(), Some(runtime_dir));
        assert_eq!(runtime_terminfo_dir(), Some(terminfo_dir));
    }

    #[test]
    fn normalize_gtk_bridge_layout_synthesizes_generic_bridge_for_existing_runtime() {
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        fs::create_dir_all(runtime_dir.join("lib")).expect("runtime lib dir");
        fs::write(
            runtime_dir.join("lib").join(LEGACY_BRIDGE_LIBRARY_NAME),
            b"legacy bridge",
        )
        .expect("legacy bridge");

        normalize_gtk_bridge_layout(&runtime_dir).expect("normalize runtime");

        assert_eq!(
            fs::read(runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME))
                .expect("generic bridge"),
            b"legacy bridge"
        );
    }

    #[test]
    fn installed_runtime_is_current_requires_generic_bridge_layout() {
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");
        fs::create_dir_all(runtime_dir.join("lib")).expect("runtime lib dir");
        fs::create_dir_all(terminfo_dir.join("g")).expect("terminfo dir");
        fs::write(
            runtime_dir.join("lib").join(LEGACY_BRIDGE_LIBRARY_NAME),
            b"legacy bridge",
        )
        .expect("legacy bridge");
        fs::write(terminfo_dir.join("g").join("ghostty"), b"terminfo").expect("terminfo");
        fs::write(
            runtime_dir.join(RUNTIME_VERSION_FILE),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("version");

        assert!(!installed_runtime_is_current(&runtime_dir));

        let build_runtime = super::build_runtime_layout().expect("build runtime layout");
        fs::copy(
            build_runtime.gtk_bridge_path,
            runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME),
        )
        .expect("valid generic bridge");
        assert!(installed_runtime_is_current(&runtime_dir));
    }

    #[test]
    fn installed_runtime_is_current_rejects_generic_bridge_without_required_symbols() {
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");
        fs::create_dir_all(runtime_dir.join("lib")).expect("runtime lib dir");
        fs::create_dir_all(terminfo_dir.join("g")).expect("terminfo dir");
        fs::write(
            runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME),
            b"not-a-real-shared-library",
        )
        .expect("invalid generic bridge");
        fs::write(terminfo_dir.join("g").join("ghostty"), b"terminfo").expect("terminfo");
        fs::write(
            runtime_dir.join(RUNTIME_VERSION_FILE),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("version");

        assert!(!gtk_bridge_library_has_required_symbols(
            &runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME)
        ));
        assert!(!installed_runtime_is_current(&runtime_dir));
    }

    #[test]
    fn legacy_bundle_path_alias_still_installs_runtime_layout() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let bundle_path = temp.path().join("ghostty-runtime.tar.xz");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");

        let bundle_source = temp.path().join("bundle-source");
        fs::create_dir_all(bundle_source.join("ghostty/lib")).expect("ghostty lib dir");
        fs::create_dir_all(bundle_source.join("ghostty/shell-integration/bash"))
            .expect("shell integration dir");
        fs::create_dir_all(bundle_source.join("terminfo/x")).expect("terminfo dir");
        fs::write(
            bundle_source
                .join("ghostty")
                .join("lib")
                .join(LEGACY_BRIDGE_LIBRARY_NAME),
            b"fake bridge",
        )
        .expect("write fake bridge");
        fs::write(
            bundle_source
                .join("ghostty")
                .join("shell-integration")
                .join("bash")
                .join("ghostty.bash"),
            b"echo ghostty",
        )
        .expect("write fake shell integration");
        fs::write(
            bundle_source
                .join("terminfo")
                .join("x")
                .join("xterm-ghostty"),
            b"fake terminfo",
        )
        .expect("write fake terminfo");
        write_bundle(&bundle_source, &bundle_path);

        let _guard = EnvGuard::set([
            (GTK_BUNDLE_PATH_ENV, None),
            (GTK_BUNDLE_URL_ENV, None),
            (GTK_DISABLE_BOOTSTRAP_ENV, None),
            (GTK_RUNTIME_DIR_ENV, None),
            (GTK_BRIDGE_PATH_ENV, None),
            (
                "TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH",
                Some(bundle_path.as_os_str()),
            ),
            ("TASKERS_GHOSTTY_RUNTIME_URL", None),
            ("TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP", None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", Some(runtime_dir.as_os_str())),
            ("TASKERS_GHOSTTY_BRIDGE_PATH", None),
            ("GHOSTTY_RESOURCES_DIR", None),
            ("XDG_DATA_HOME", None),
        ]);

        let result = ensure_runtime_installed().expect("runtime install");
        assert_eq!(
            result,
            Some(RuntimeBootstrap {
                runtime_dir: runtime_dir.clone(),
            })
        );
        assert!(
            runtime_dir
                .join("lib")
                .join(GTK_BRIDGE_LIBRARY_NAME)
                .exists()
        );
        assert!(
            runtime_dir
                .join("lib")
                .join(LEGACY_BRIDGE_LIBRARY_NAME)
                .exists()
        );
        assert_eq!(
            runtime_gtk_bridge_path(),
            Some(runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME))
        );
        assert_eq!(runtime_resources_dir(), Some(runtime_dir));
        assert_eq!(runtime_terminfo_dir(), Some(terminfo_dir));
    }

    #[test]
    fn configure_runtime_environment_uses_explicit_runtime_dir() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        fs::create_dir_all(&runtime_dir).expect("runtime dir");

        let _guard = EnvGuard::set([
            (GTK_RUNTIME_DIR_ENV, None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", Some(runtime_dir.as_os_str())),
            ("GHOSTTY_RESOURCES_DIR", None),
        ]);

        super::configure_runtime_environment();

        assert_eq!(
            env::var_os("GHOSTTY_RESOURCES_DIR").map(std::path::PathBuf::from),
            Some(runtime_dir.clone())
        );
        assert_eq!(
            env::var_os(GTK_RUNTIME_DIR_ENV).map(std::path::PathBuf::from),
            Some(runtime_dir.clone())
        );
        assert_eq!(
            env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR").map(std::path::PathBuf::from),
            Some(runtime_dir)
        );
    }

    #[test]
    fn configure_runtime_environment_prefers_generic_runtime_dir_without_legacy_export() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        fs::create_dir_all(&runtime_dir).expect("runtime dir");

        let _guard = EnvGuard::set([
            ("TASKERS_GHOSTTY_RUNTIME_DIR", None),
            (GTK_RUNTIME_DIR_ENV, Some(runtime_dir.as_os_str())),
            (GTK_BRIDGE_PATH_ENV, None),
            ("GHOSTTY_RESOURCES_DIR", None),
        ]);

        super::configure_runtime_environment();

        assert_eq!(
            env::var_os("GHOSTTY_RESOURCES_DIR").map(std::path::PathBuf::from),
            Some(runtime_dir.clone())
        );
        assert_eq!(
            env::var_os(GTK_RUNTIME_DIR_ENV).map(std::path::PathBuf::from),
            Some(runtime_dir)
        );
        assert_eq!(
            env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR").map(std::path::PathBuf::from),
            None
        );
    }

    #[test]
    fn generic_disable_bootstrap_alias_skips_runtime_install() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let bundle_path = temp.path().join("ghostty-runtime.tar.xz");
        fs::write(&bundle_path, b"not-used").expect("bundle placeholder");

        let _guard = EnvGuard::set([
            (GTK_DISABLE_BOOTSTRAP_ENV, Some(std::ffi::OsStr::new("1"))),
            (GTK_BUNDLE_PATH_ENV, Some(bundle_path.as_os_str())),
            (GTK_BUNDLE_URL_ENV, None),
            (GTK_RUNTIME_DIR_ENV, None),
            (GTK_BRIDGE_PATH_ENV, None),
            ("TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP", None),
            ("TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH", None),
            ("TASKERS_GHOSTTY_RUNTIME_URL", None),
        ]);

        assert_eq!(ensure_runtime_installed().expect("runtime install"), None);
    }

    #[test]
    fn runtime_terminfo_dir_follows_explicit_runtime_dir() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");
        fs::create_dir_all(&runtime_dir).expect("runtime dir");
        fs::create_dir_all(terminfo_dir.join("x")).expect("terminfo dir");
        fs::write(
            terminfo_dir.join("x").join("xterm-ghostty"),
            b"fake terminfo",
        )
        .expect("write fake terminfo");

        let _guard = EnvGuard::set([
            (GTK_RUNTIME_DIR_ENV, None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", Some(runtime_dir.as_os_str())),
            ("TERMINFO", None),
            ("XDG_DATA_HOME", None),
        ]);

        assert_eq!(runtime_terminfo_dir(), Some(terminfo_dir));
    }

    #[test]
    fn installed_executables_do_not_use_build_runtime_directly() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        assert!(!use_build_runtime_directly_for(Some(Path::new(
            "/home/notes/.local/share/cargo/bin/taskers-gtk",
        ))));
        assert!(use_build_runtime_directly_for(Some(
            &super::repo_target_dir().join("debug").join("taskers-gtk"),
        )));
    }

    #[test]
    fn installed_executables_prefer_managed_runtime_paths() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");
        fs::create_dir_all(runtime_dir.join("lib")).expect("runtime lib dir");
        fs::create_dir_all(terminfo_dir.join("x")).expect("terminfo dir");
        fs::write(
            runtime_dir.join("lib").join("libtaskers_ghostty_bridge.so"),
            b"bridge",
        )
        .expect("bridge");
        fs::write(runtime_dir.join("theme.txt"), b"theme").expect("theme");
        fs::write(terminfo_dir.join("x").join("xterm-ghostty"), b"terminfo").expect("terminfo");

        let _guard = EnvGuard::set([
            ("XDG_DATA_HOME", Some(temp.path().as_os_str())),
            (GTK_RUNTIME_DIR_ENV, None),
            (GTK_BRIDGE_PATH_ENV, None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", None),
            ("TASKERS_GHOSTTY_BRIDGE_PATH", None),
            ("GHOSTTY_RESOURCES_DIR", None),
            ("TERMINFO", None),
        ]);

        let installed_exe = Path::new("/home/notes/.local/share/cargo/bin/taskers-gtk");
        assert_eq!(
            runtime_resources_dir_for(Some(installed_exe)),
            Some(runtime_dir.clone())
        );
        assert!(
            runtime_dir
                .join("lib")
                .join(GTK_BRIDGE_LIBRARY_NAME)
                .exists()
        );
        assert_eq!(
            runtime_gtk_bridge_path_for(Some(installed_exe)),
            Some(runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME))
        );
        assert_eq!(
            runtime_terminfo_dir_for(Some(installed_exe)),
            Some(terminfo_dir)
        );
    }

    #[test]
    fn installed_executables_prefer_generic_bridge_library_when_available() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let runtime_dir = temp.path().join("taskers").join("ghostty");
        let terminfo_dir = temp.path().join("taskers").join("terminfo");
        fs::create_dir_all(runtime_dir.join("lib")).expect("runtime lib dir");
        fs::create_dir_all(terminfo_dir.join("x")).expect("terminfo dir");
        fs::write(
            runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME),
            b"bridge",
        )
        .expect("bridge");
        fs::write(terminfo_dir.join("x").join("xterm-ghostty"), b"terminfo").expect("terminfo");

        let _guard = EnvGuard::set([
            ("XDG_DATA_HOME", Some(temp.path().as_os_str())),
            (GTK_RUNTIME_DIR_ENV, None),
            (GTK_BRIDGE_PATH_ENV, None),
            ("TASKERS_GHOSTTY_RUNTIME_DIR", None),
            ("TASKERS_GHOSTTY_BRIDGE_PATH", None),
            ("GHOSTTY_RESOURCES_DIR", None),
            ("TERMINFO", None),
        ]);

        let installed_exe = Path::new("/home/notes/.local/share/cargo/bin/taskers-gtk");
        assert_eq!(
            runtime_gtk_bridge_path_for(Some(installed_exe)),
            Some(runtime_dir.join("lib").join(GTK_BRIDGE_LIBRARY_NAME))
        );
    }

    #[test]
    fn explicit_generic_bridge_env_overrides_runtime_lookup() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let explicit_bridge = temp.path().join(GTK_BRIDGE_LIBRARY_NAME);
        fs::write(&explicit_bridge, b"bridge").expect("bridge");

        let _guard = EnvGuard::set([
            (GTK_BRIDGE_PATH_ENV, Some(explicit_bridge.as_os_str())),
            ("TASKERS_GHOSTTY_BRIDGE_PATH", None),
        ]);

        assert_eq!(runtime_gtk_bridge_path_for(None), Some(explicit_bridge));
    }

    #[test]
    fn stage_build_runtime_layout_copies_bridge_resources_and_terminfo() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let build_root = temp.path().join("build");
        let resources_dir = build_root.join("share").join("ghostty");
        let terminfo_dir = build_root.join("share").join("terminfo");
        let gtk_bridge_path = build_root.join("lib").join(GTK_BRIDGE_LIBRARY_NAME);
        fs::create_dir_all(resources_dir.join("shell-integration")).expect("resources dir");
        fs::create_dir_all(terminfo_dir.join("g")).expect("terminfo dir");
        fs::create_dir_all(gtk_bridge_path.parent().expect("gtk bridge dir"))
            .expect("gtk bridge dir");
        fs::write(resources_dir.join("theme.txt"), b"theme").expect("theme");
        fs::write(
            resources_dir.join("shell-integration").join("ghostty.bash"),
            b"shell",
        )
        .expect("shell integration");
        fs::write(&gtk_bridge_path, b"bridge").expect("bridge");
        fs::write(terminfo_dir.join("g").join("ghostty"), b"terminfo").expect("terminfo");

        let staging_root = temp.path().join("staging");
        fs::create_dir_all(&staging_root).expect("staging root");
        stage_build_runtime_layout(
            &BuildRuntimeLayout {
                gtk_bridge_path: gtk_bridge_path.clone(),
                resources_dir: resources_dir.clone(),
                terminfo_dir: terminfo_dir.clone(),
            },
            &staging_root,
        )
        .expect("stage build runtime");

        assert_eq!(
            fs::read(staging_root.join("ghostty").join("theme.txt")).expect("staged theme"),
            b"theme"
        );
        assert_eq!(
            fs::read(
                staging_root
                    .join("ghostty")
                    .join("shell-integration")
                    .join("ghostty.bash"),
            )
            .expect("staged shell integration"),
            b"shell"
        );
        assert_eq!(
            fs::read(
                staging_root
                    .join("ghostty")
                    .join("lib")
                    .join("libtaskers_ghostty_bridge.so"),
            )
            .expect("staged gtk bridge"),
            b"bridge"
        );
        assert_eq!(
            fs::read(
                staging_root
                    .join("ghostty")
                    .join("lib")
                    .join(GTK_BRIDGE_LIBRARY_NAME),
            )
            .expect("staged generic bridge"),
            b"bridge"
        );
        assert_eq!(
            fs::read(staging_root.join("terminfo").join("g").join("ghostty"))
                .expect("staged terminfo"),
            b"terminfo"
        );
    }

    #[test]
    fn stage_build_runtime_layout_accepts_legacy_bridge_source_path() {
        let _lock = RUNTIME_ENV_LOCK.lock().expect("runtime env lock");
        let temp = tempdir().expect("tempdir");
        let build_root = temp.path().join("build");
        let resources_dir = build_root.join("share").join("ghostty");
        let terminfo_dir = build_root.join("share").join("terminfo");
        let gtk_bridge_path = build_root.join("lib").join(LEGACY_BRIDGE_LIBRARY_NAME);
        fs::create_dir_all(&resources_dir).expect("resources dir");
        fs::create_dir_all(terminfo_dir.join("g")).expect("terminfo dir");
        fs::create_dir_all(gtk_bridge_path.parent().expect("gtk bridge dir"))
            .expect("gtk bridge dir");
        fs::write(&gtk_bridge_path, b"bridge").expect("bridge");
        fs::write(terminfo_dir.join("g").join("ghostty"), b"terminfo").expect("terminfo");

        let staging_root = temp.path().join("staging");
        fs::create_dir_all(&staging_root).expect("staging root");
        stage_build_runtime_layout(
            &BuildRuntimeLayout {
                gtk_bridge_path,
                resources_dir,
                terminfo_dir,
            },
            &staging_root,
        )
        .expect("stage build runtime");

        assert_eq!(
            fs::read(
                staging_root
                    .join("ghostty")
                    .join("lib")
                    .join(GTK_BRIDGE_LIBRARY_NAME),
            )
            .expect("staged generic bridge"),
            b"bridge"
        );
        assert_eq!(
            fs::read(
                staging_root
                    .join("ghostty")
                    .join("lib")
                    .join(LEGACY_BRIDGE_LIBRARY_NAME),
            )
            .expect("staged legacy bridge"),
            b"bridge"
        );
    }

    fn raw_tar_with_file(path: &str, payload: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..path.len()].copy_from_slice(path.as_bytes());
        header[100..108].copy_from_slice(b"0000644\0");
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        let size = format!("{:011o}\0", payload.len());
        header[124..136].copy_from_slice(size.as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
        let checksum = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(checksum.as_bytes());

        let mut tar = Vec::from(header);
        tar.extend_from_slice(payload);
        let padding = (512 - (payload.len() % 512)) % 512;
        tar.resize(tar.len() + padding, 0);
        tar.resize(tar.len() + 1024, 0);
        tar
    }

    fn write_bundle(source_dir: &Path, bundle_path: &Path) {
        let file = fs::File::create(bundle_path).expect("create bundle");
        let encoder = XzEncoder::new(file, 9);
        let mut builder = Builder::new(encoder);
        builder
            .append_dir_all("ghostty", source_dir.join("ghostty"))
            .expect("append ghostty");
        builder
            .append_dir_all("terminfo", source_dir.join("terminfo"))
            .expect("append terminfo");
        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish xz");
    }

    struct EnvGuard {
        saved: Vec<(String, Option<std::ffi::OsString>)>,
    }

    impl EnvGuard {
        fn set<const N: usize>(entries: [(&str, Option<&std::ffi::OsStr>); N]) -> Self {
            let mut saved = Vec::with_capacity(N);
            for (key, value) in entries {
                saved.push((key.to_string(), env::var_os(key)));
                unsafe {
                    match value {
                        Some(value) => env::set_var(key, value),
                        None => env::remove_var(key),
                    }
                }
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..).rev() {
                unsafe {
                    match value {
                        Some(value) => env::set_var(&key, value),
                        None => env::remove_var(&key),
                    }
                }
            }
        }
    }
}
