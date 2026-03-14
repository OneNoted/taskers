use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

use tar::Archive;
use thiserror::Error;
use xz2::read::XzDecoder;

const BRIDGE_LIBRARY_NAME: &str = "libtaskers_ghostty_bridge.so";
const RUNTIME_VERSION_FILE: &str = ".taskers-runtime-version";
const TERMINFO_GHOSTTY_PATH: &str = "g/ghostty";
const TERMINFO_XTERM_GHOSTTY_PATH: &str = "x/xterm-ghostty";
const BUNDLE_PATH_ENV: &str = "TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH";
const BUNDLE_URL_ENV: &str = "TASKERS_GHOSTTY_RUNTIME_URL";
const DISABLE_BOOTSTRAP_ENV: &str = "TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP";

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
    #[error("Ghostty runtime bundle missing required file {path}")]
    MissingBundlePath { path: &'static str },
    #[error("failed to write Ghostty runtime version marker at {path}: {message}")]
    WriteVersion { path: PathBuf, message: String },
}

pub fn ensure_runtime_installed() -> Result<Option<RuntimeBootstrap>, RuntimeBootstrapError> {
    if env::var_os(DISABLE_BOOTSTRAP_ENV).is_some() {
        return Ok(None);
    }

    let bundle_override =
        env::var_os(BUNDLE_PATH_ENV).is_some() || env::var_os(BUNDLE_URL_ENV).is_some();
    if !bundle_override && build_runtime_ready() {
        return Ok(None);
    }

    let Some(runtime_dir) = installed_runtime_dir() else {
        return Ok(None);
    };
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

    let install_result = if let Some(bundle_path) = env::var_os(BUNDLE_PATH_ENV).map(PathBuf::from)
    {
        let file =
            fs::File::open(&bundle_path).map_err(|error| RuntimeBootstrapError::OpenBundle {
                path: bundle_path.clone(),
                message: error.to_string(),
            })?;
        unpack_bundle(file, &staging_root)
    } else {
        let url = env::var(BUNDLE_URL_ENV).unwrap_or_else(|_| default_runtime_bundle_url());
        let response =
            ureq::get(&url)
                .call()
                .map_err(|error| RuntimeBootstrapError::DownloadBundle {
                    url: url.clone(),
                    message: error.to_string(),
                })?;
        unpack_bundle(response.into_reader(), &staging_root).map_err(|error| match error {
            RuntimeBootstrapError::UnpackBundle { .. }
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

    if let Some(path) = explicit_runtime_dir().filter(|path| path.exists()) {
        unsafe {
            env::set_var("GHOSTTY_RESOURCES_DIR", &path);
        }
        return;
    }

    if let Some(path) = build_runtime_resources_dir() {
        unsafe {
            env::set_var("GHOSTTY_RESOURCES_DIR", &path);
        }
        return;
    }

    if let Some(path) = default_installed_runtime_dir().filter(|path| path.exists()) {
        unsafe {
            env::set_var("GHOSTTY_RESOURCES_DIR", &path);
        }
    }
}

pub fn runtime_resources_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os("GHOSTTY_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = explicit_runtime_dir().filter(|path| path.exists()) {
        return Some(path);
    }

    if let Some(path) = build_runtime_resources_dir() {
        return Some(path);
    }

    default_installed_runtime_dir().filter(|path| path.exists())
}

pub fn runtime_bridge_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("TASKERS_GHOSTTY_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = explicit_runtime_dir()
        .map(|root| root.join("lib").join(BRIDGE_LIBRARY_NAME))
        .filter(|path| path.exists())
    {
        return Some(path);
    }

    if let Some(path) = build_runtime_bridge_path() {
        return Some(path);
    }

    default_installed_runtime_dir()
        .map(|root| root.join("lib").join(BRIDGE_LIBRARY_NAME))
        .filter(|path| path.exists())
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
    if !ghostty_dir.join("lib").join(BRIDGE_LIBRARY_NAME).exists() {
        return Err(RuntimeBootstrapError::MissingBundlePath {
            path: "ghostty/lib/libtaskers_ghostty_bridge.so",
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

fn installed_runtime_is_current(runtime_dir: &Path) -> bool {
    if !runtime_dir.join("lib").join(BRIDGE_LIBRARY_NAME).exists() {
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

fn build_runtime_ready() -> bool {
    build_runtime_bridge_path().is_some() && build_runtime_resources_dir().is_some()
}

fn build_runtime_bridge_path() -> Option<PathBuf> {
    option_env!("TASKERS_GHOSTTY_BUILD_BRIDGE_PATH")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn build_runtime_resources_dir() -> Option<PathBuf> {
    option_env!("TASKERS_GHOSTTY_BUILD_RESOURCES_DIR")
        .map(PathBuf::from)
        .filter(|path| path.exists())
}

fn installed_runtime_dir() -> Option<PathBuf> {
    explicit_runtime_dir().or_else(default_installed_runtime_dir)
}

fn explicit_runtime_dir() -> Option<PathBuf> {
    env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR").map(PathBuf::from)
}

fn default_installed_runtime_dir() -> Option<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .map(|path| path.join("taskers").join("ghostty"))
    {
        return Some(path);
    }

    env::var_os("HOME").map(PathBuf::from).map(|path| {
        path.join(".local")
            .join("share")
            .join("taskers")
            .join("ghostty")
    })
}

fn default_runtime_bundle_url() -> String {
    format!(
        "https://github.com/OneNoted/taskers/releases/download/v{version}/taskers-ghostty-runtime-v{version}-{target}.tar.xz",
        version = env!("CARGO_PKG_VERSION"),
        target = option_env!("TASKERS_BUILD_TARGET").unwrap_or("x86_64-unknown-linux-gnu"),
    )
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
        RUNTIME_VERSION_FILE, RuntimeBootstrap, ensure_runtime_installed, runtime_bridge_path,
        runtime_resources_dir,
    };
    use std::{env, fs, path::Path};
    use tar::Builder;
    use tempfile::tempdir;
    use xz2::write::XzEncoder;

    #[test]
    fn local_bundle_bootstrap_installs_runtime_layout() {
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
                .join("libtaskers_ghostty_bridge.so"),
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
            (
                "TASKERS_GHOSTTY_RUNTIME_BUNDLE_PATH",
                Some(bundle_path.as_os_str()),
            ),
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
                .join("libtaskers_ghostty_bridge.so")
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
            runtime_bridge_path(),
            Some(runtime_dir.join("lib").join("libtaskers_ghostty_bridge.so"))
        );
        assert_eq!(runtime_resources_dir(), Some(runtime_dir));
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
