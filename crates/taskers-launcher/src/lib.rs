#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
compile_error!(
    "taskers on crates.io currently supports x86_64 Linux only. Mainline macOS support is not shipped from this repo root."
);

use std::{
    collections::BTreeMap,
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use taskers_paths::default_release_install_root;
use xz2::read::XzDecoder;

const INSTALL_ROOT_ENV: &str = "TASKERS_INSTALL_ROOT";
const MANIFEST_URL_ENV: &str = "TASKERS_RELEASE_MANIFEST_URL";
const SKIP_DESKTOP_INTEGRATION_ENV: &str = "TASKERS_SKIP_DESKTOP_INTEGRATION";

pub fn run() -> Result<ExitStatus> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let installation = ManagedInstallation::ensure_installed(env!("CARGO_PKG_VERSION"))?;
    installation.install_linux_user_assets()?;
    installation.launch(&args)
}

#[derive(Debug, Deserialize, Serialize)]
struct ReleaseManifest {
    version: String,
    artifacts: BTreeMap<String, ReleaseArtifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReleaseArtifact {
    kind: ArtifactKind,
    url: String,
    sha256: String,
    #[serde(default)]
    size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    #[serde(rename = "linux_bundle_v1")]
    LinuxBundleV1,
}

#[derive(Debug)]
struct ManagedInstallation {
    target_triple: String,
    version: String,
    bundle_root: PathBuf,
}

impl ManagedInstallation {
    fn ensure_installed(version: &str) -> Result<Self> {
        let target_triple = current_target_triple()?;
        let install_root = env::var_os(INSTALL_ROOT_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(default_release_install_root);
        let bundle_root = bundle_root(&install_root, version, target_triple);
        let installation = Self {
            target_triple: target_triple.to_string(),
            version: version.to_string(),
            bundle_root,
        };

        if installation.is_complete() {
            return Ok(installation);
        }

        let manifest = installation.load_manifest()?;
        let artifact = manifest
            .artifacts
            .get(&installation.target_triple)
            .with_context(|| {
                format!(
                    "release manifest does not contain an artifact for {}",
                    installation.target_triple
                )
            })?;
        installation.install_artifact(artifact)?;
        Ok(installation)
    }

    fn launch(&self, args: &[OsString]) -> Result<ExitStatus> {
        let executable = self.executable_path();
        let mut command = Command::new(&executable);
        command.args(args);
        command.env("TASKERS_CTL_PATH", self.taskersctl_path());
        command.env("GHOSTTY_GTK_RUNTIME_DIR", self.ghostty_resources_path());
        command.env("TASKERS_GHOSTTY_RUNTIME_DIR", self.ghostty_resources_path());
        command.env("GHOSTTY_RESOURCES_DIR", self.ghostty_resources_path());
        command.env("TERMINFO", self.terminfo_path());
        command.env("GHOSTTY_GTK_DISABLE_RUNTIME_BOOTSTRAP", "1");
        command.env("TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP", "1");

        command
            .status()
            .with_context(|| format!("failed to launch {}", executable.display()))
    }

    fn load_manifest(&self) -> Result<ReleaseManifest> {
        let manifest_url =
            env::var(MANIFEST_URL_ENV).unwrap_or_else(|_| default_manifest_url(&self.version));
        let manifest_bytes = read_source_bytes(&manifest_url)
            .with_context(|| format!("failed to load release manifest from {manifest_url}"))?;
        let manifest: ReleaseManifest = serde_json::from_slice(&manifest_bytes)
            .with_context(|| format!("failed to decode release manifest from {manifest_url}"))?;
        if manifest.version != self.version {
            bail!(
                "release manifest version {} does not match launcher version {}",
                manifest.version,
                self.version
            );
        }
        Ok(manifest)
    }

    fn install_artifact(&self, artifact: &ReleaseArtifact) -> Result<()> {
        validate_artifact_kind(artifact.kind)?;

        let version_root = self
            .bundle_root
            .parent()
            .ok_or_else(|| anyhow!("bundle root {} has no parent", self.bundle_root.display()))?;
        fs::create_dir_all(version_root)
            .with_context(|| format!("failed to create {}", version_root.display()))?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let staging_root =
            version_root.join(format!(".install-{}-{timestamp}", std::process::id()));
        if staging_root.exists() {
            remove_path(&staging_root)?;
        }
        fs::create_dir_all(&staging_root)
            .with_context(|| format!("failed to create {}", staging_root.display()))?;

        let download_path = staging_root.join("artifact.download");
        fetch_source_to_path(&artifact.url, &download_path).with_context(|| {
            format!(
                "failed to download release artifact for {} from {}",
                self.target_triple, artifact.url
            )
        })?;

        if let Some(expected_size) = artifact.size_bytes {
            let actual_size = fs::metadata(&download_path)
                .with_context(|| format!("failed to stat {}", download_path.display()))?
                .len();
            if actual_size != expected_size {
                bail!(
                    "artifact size mismatch for {}: expected {}, got {}",
                    artifact.url,
                    expected_size,
                    actual_size
                );
            }
        }

        let digest = sha256_path(&download_path)?;
        if digest != artifact.sha256.to_ascii_lowercase() {
            bail!(
                "artifact checksum mismatch for {}: expected {}, got {}",
                artifact.url,
                artifact.sha256,
                digest
            );
        }

        let unpack_root = staging_root.join("unpacked");
        fs::create_dir_all(&unpack_root)
            .with_context(|| format!("failed to create {}", unpack_root.display()))?;
        unpack_linux_bundle(&download_path, &unpack_root)?;

        if !validate_bundle_layout(&unpack_root) {
            bail!(
                "artifact {} did not unpack the expected layout into {}",
                artifact.url,
                unpack_root.display()
            );
        }

        if self.bundle_root.exists() {
            remove_path(&self.bundle_root)?;
        }
        if let Some(parent) = self.bundle_root.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::rename(&unpack_root, &self.bundle_root).with_context(|| {
            format!(
                "failed to move {} to {}",
                unpack_root.display(),
                self.bundle_root.display()
            )
        })?;
        remove_path(&staging_root)?;

        Ok(())
    }

    fn install_linux_user_assets(&self) -> Result<()> {
        if env::var_os(SKIP_DESKTOP_INTEGRATION_ENV).is_some() {
            return Ok(());
        }

        let Some(launcher) = desktop_launcher_path()? else {
            return Ok(());
        };
        let xdg_data_home = xdg_data_home()?;
        let applications_dir = xdg_data_home.join("applications");
        let icons_dir = xdg_data_home
            .join("icons")
            .join("hicolor")
            .join("scalable")
            .join("apps");
        let xdg_bin_home = xdg_bin_home()?;
        fs::create_dir_all(&applications_dir)
            .with_context(|| format!("failed to create {}", applications_dir.display()))?;
        fs::create_dir_all(&icons_dir)
            .with_context(|| format!("failed to create {}", icons_dir.display()))?;
        fs::create_dir_all(&xdg_bin_home)
            .with_context(|| format!("failed to create {}", xdg_bin_home.display()))?;

        let desktop_launcher = xdg_bin_home.join("taskers-desktop-launch");
        write_executable(
            &desktop_launcher,
            &desktop_launch_wrapper_contents(&launcher),
        )?;

        let desktop_entry_path = applications_dir.join("dev.taskers.app.desktop");
        let cargo_bin_home = cargo_bin_home();
        let legacy_desktop_launcher = cargo_bin_home
            .as_deref()
            .map(|path| path.join("taskers-gtk-desktop-launch"));
        let desktop_entry = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/taskers.desktop.in"
        ))
        .replace("{{EXEC}}", &desktop_exec(&desktop_launcher));
        if should_update_desktop_entry(
            &desktop_entry_path,
            &desktop_launcher,
            &launcher,
            legacy_desktop_launcher.as_deref(),
        )? {
            fs::write(&desktop_entry_path, desktop_entry)
                .with_context(|| format!("failed to write {}", applications_dir.display()))?;
        }
        fs::write(
            icons_dir.join("taskers.svg"),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/taskers.svg")),
        )
        .with_context(|| format!("failed to write {}", icons_dir.display()))?;

        remove_legacy_desktop_integration(legacy_desktop_launcher.as_deref())?;

        let notify_path = xdg_bin_home.join("taskers-notify");
        write_executable(
            &notify_path,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/taskers-notify.sh"
            )),
        )?;

        refresh_desktop_indexes(&applications_dir);
        Ok(())
    }

    fn is_complete(&self) -> bool {
        validate_bundle_layout(&self.bundle_root)
    }

    fn executable_path(&self) -> PathBuf {
        self.bundle_root.join("bin").join("taskers")
    }

    fn taskersctl_path(&self) -> PathBuf {
        self.bundle_root.join("bin").join("taskersctl")
    }

    fn ghostty_resources_path(&self) -> PathBuf {
        self.bundle_root.join("ghostty")
    }

    fn terminfo_path(&self) -> PathBuf {
        self.bundle_root.join("terminfo")
    }
}

fn validate_artifact_kind(kind: ArtifactKind) -> Result<()> {
    match kind {
        ArtifactKind::LinuxBundleV1 => Ok(()),
    }
}

fn validate_bundle_layout(bundle_root: &Path) -> bool {
    bundle_root.join("bin").join("taskers").is_file()
        && bundle_root.join("bin").join("taskersctl").is_file()
        && bundle_root.join("bin").join("taskers-terminald").is_file()
        && bundle_root.join("ghostty").is_dir()
        && (bundle_root
            .join("ghostty")
            .join("lib")
            .join("libghostty_gtk.so")
            .is_file()
            || bundle_root
                .join("ghostty")
                .join("lib")
                .join("libtaskers_ghostty_bridge.so")
                .is_file())
        && bundle_root.join("ghostty").join("themes").is_dir()
        && bundle_root.join("terminfo").is_dir()
}

fn current_target_triple() -> Result<&'static str> {
    match env::consts::ARCH {
        "x86_64" => Ok("x86_64-unknown-linux-gnu"),
        _ => bail!(
            "unsupported Linux architecture for the published taskers launcher: {}",
            env::consts::ARCH
        ),
    }
}

fn bundle_root(install_root: &Path, version: &str, target_triple: &str) -> PathBuf {
    install_root.join(version).join(target_triple)
}

fn default_manifest_url(version: &str) -> String {
    let repository = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    format!("{repository}/releases/download/v{version}/taskers-manifest-v{version}.json")
}

fn read_source_bytes(source: &str) -> Result<Vec<u8>> {
    if let Some(path) = file_url_to_path(source) {
        return fs::read(&path).with_context(|| format!("failed to read {}", path.display()));
    }
    if let Some(path) = local_path_source(source) {
        return fs::read(&path).with_context(|| format!("failed to read {}", path.display()));
    }

    let response = ureq::get(source)
        .call()
        .with_context(|| format!("failed to fetch {source}"))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read response body from {source}"))?;
    Ok(bytes)
}

fn fetch_source_to_path(source: &str, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if let Some(path) = file_url_to_path(source).or_else(|| local_path_source(source)) {
        fs::copy(&path, destination).with_context(|| {
            format!(
                "failed to copy {} to {}",
                path.display(),
                destination.display()
            )
        })?;
        return Ok(());
    }

    let response = ureq::get(source)
        .call()
        .with_context(|| format!("failed to fetch {source}"))?;
    let mut reader = response.into_reader();
    let mut file = fs::File::create(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;
    io::copy(&mut reader, &mut file)
        .with_context(|| format!("failed to write {}", destination.display()))?;
    Ok(())
}

fn file_url_to_path(value: &str) -> Option<PathBuf> {
    value.strip_prefix("file://").map(PathBuf::from)
}

fn local_path_source(value: &str) -> Option<PathBuf> {
    if value.contains("://") {
        None
    } else {
        Some(PathBuf::from(value))
    }
}

fn sha256_path(path: &Path) -> Result<String> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn unpack_linux_bundle(archive_path: &Path, destination: &Path) -> Result<()> {
    let file = fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = XzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination)
        .with_context(|| format!("failed to unpack {}", archive_path.display()))
}

fn remove_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.file_type().is_symlink() || metadata.is_file() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    } else {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn xdg_bin_home() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_BIN_HOME").map(PathBuf::from) {
        return Ok(path);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and XDG_BIN_HOME is unavailable")?;
    Ok(home.join(".local").join("bin"))
}

fn xdg_data_home() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_DATA_HOME").map(PathBuf::from) {
        return Ok(path);
    }
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set and XDG_DATA_HOME is unavailable")?;
    Ok(home.join(".local").join("share"))
}

fn desktop_exec(path: &Path) -> String {
    let raw = path.display().to_string();
    raw.replace('\\', "\\\\").replace(' ', "\\ ")
}

fn shell_single_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\"'\"'"))
}

fn desktop_launch_wrapper_contents(target: &Path) -> String {
    format!(
        "#!/bin/sh\nset -eu\nlog_dir=\"${{XDG_CACHE_HOME:-$HOME/.cache}}/taskers\"\nmkdir -p \"$log_dir\"\n\nfocus_taskers_window_niri_once() {{\n  command -v niri >/dev/null 2>&1 || return 1\n  command -v jq >/dev/null 2>&1 || return 1\n\n  window_id=\"$(niri msg -j windows 2>/dev/null | jq -r 'first(.[] | select(.app_id == \"dev.taskers.app\") | .id) // empty' 2>/dev/null)\"\n  [ -n \"$window_id\" ] || return 1\n  niri msg action focus-window --id \"$window_id\" >/dev/null 2>&1\n}}\n\nif focus_taskers_window_niri_once; then\n  exit 0\nfi\n\n/usr/bin/setsid -f {target} --diagnostic-log \"$log_dir/desktop-launch-diagnostics.log\" >>\"$log_dir/desktop-launch.log\" 2>&1\n",
        target = shell_single_quote(target),
    )
}

fn should_update_desktop_entry(
    path: &Path,
    desktop_launcher: &Path,
    launcher: &Path,
    legacy_desktop_launcher: Option<&Path>,
) -> Result<bool> {
    let Ok(existing) = fs::read_to_string(path) else {
        return Ok(true);
    };
    let Some(existing_exec) = existing
        .lines()
        .find_map(|line| line.strip_prefix("Exec="))
        .map(str::trim)
    else {
        return Ok(true);
    };

    let mut managed_execs = vec![desktop_exec(desktop_launcher), desktop_exec(launcher)];
    if let Some(legacy_desktop_launcher) = legacy_desktop_launcher {
        managed_execs.push(desktop_exec(legacy_desktop_launcher));
    }

    Ok(managed_execs
        .iter()
        .any(|candidate| candidate == existing_exec))
}

fn remove_legacy_desktop_integration(legacy_desktop_launcher: Option<&Path>) -> Result<bool> {
    let Some(legacy_desktop_launcher) = legacy_desktop_launcher else {
        return Ok(false);
    };

    if fs::symlink_metadata(legacy_desktop_launcher).is_err() {
        return Ok(false);
    }

    remove_path(legacy_desktop_launcher)?;
    Ok(true)
}

fn write_executable(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)
            .with_context(|| format!("failed to stat {}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .with_context(|| format!("failed to chmod {}", path.display()))?;
    }

    Ok(())
}

fn desktop_launcher_path() -> Result<Option<PathBuf>> {
    let current_exe = env::current_exe().context("failed to resolve current launcher path")?;

    if let Some(path_launcher) = path_taskers_executable(&current_exe, env::var_os("PATH")) {
        return Ok(Some(path_launcher));
    }

    if launcher_path_looks_installed(&current_exe) {
        return Ok(Some(current_exe));
    }

    Ok(None)
}

fn path_taskers_executable(current_exe: &Path, path_env: Option<OsString>) -> Option<PathBuf> {
    let path_env = path_env?;
    let current_exe = fs::canonicalize(current_exe).ok()?;

    env::split_paths(&path_env).find_map(|entry| {
        let candidate = entry.join("taskers");
        let candidate_exe = fs::canonicalize(&candidate).ok()?;
        (candidate_exe == current_exe).then_some(candidate)
    })
}

fn launcher_path_looks_installed(current_exe: &Path) -> bool {
    let Some(parent) = current_exe.parent() else {
        return false;
    };

    if xdg_bin_home().ok().as_deref() == Some(parent) {
        return true;
    }

    matches!(
        parent,
        p if p == Path::new("/usr/local/bin")
            || p == Path::new("/usr/bin")
            || p == Path::new("/bin")
    )
}

fn cargo_bin_home() -> Option<PathBuf> {
    env::var_os("CARGO_INSTALL_ROOT")
        .map(PathBuf::from)
        .map(|path| path.join("bin"))
        .or_else(|| {
            env::var_os("CARGO_HOME")
                .map(PathBuf::from)
                .map(|path| path.join("bin"))
        })
        .or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".cargo").join("bin"))
        })
}

fn refresh_desktop_indexes(applications_dir: &Path) {
    run_if_available("update-desktop-database", [applications_dir.as_os_str()]);
}

fn run_if_available<I, S>(program: &str, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let Some(path) = env::var_os("PATH") else {
        return;
    };
    let Some(resolved) = env::split_paths(&path)
        .map(|entry| entry.join(program))
        .find(|candidate| candidate.exists())
    else {
        return;
    };

    let mut command = Command::new(resolved);
    for arg in args {
        command.arg(arg);
    }
    let _ = command.status();
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactKind, ManagedInstallation, ReleaseArtifact, ReleaseManifest, bundle_root,
        current_target_triple, default_manifest_url, desktop_exec, desktop_launch_wrapper_contents,
        launcher_path_looks_installed, path_taskers_executable, remove_legacy_desktop_integration,
        sha256_path, should_update_desktop_entry,
    };
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{
        collections::BTreeMap,
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
    };
    use tar::Builder;
    use tempfile::tempdir;
    use xz2::write::XzEncoder;

    #[test]
    fn default_manifest_url_uses_exact_version() {
        let version = env!("CARGO_PKG_VERSION");
        let url = default_manifest_url(version);
        assert!(url.ends_with(&format!(
            "/releases/download/v{version}/taskers-manifest-v{version}.json"
        )));
    }

    #[test]
    fn bundle_roots_match_linux_layout() {
        let root = PathBuf::from("/tmp/taskers");
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            bundle_root(&root, version, "x86_64-unknown-linux-gnu"),
            PathBuf::from(format!("/tmp/taskers/{version}/x86_64-unknown-linux-gnu"))
        );
    }

    #[test]
    fn linux_bundle_install_from_local_manifest() {
        let temp = tempdir().expect("tempdir");
        let install_root = temp.path().join("install");
        let bundle_dir = temp.path().join("bundle");
        fs::create_dir_all(bundle_dir.join("bin")).expect("bin dir");
        fs::create_dir_all(bundle_dir.join("ghostty").join("lib")).expect("ghostty dir");
        fs::create_dir_all(bundle_dir.join("ghostty").join("themes")).expect("themes dir");
        fs::create_dir_all(bundle_dir.join("terminfo").join("g")).expect("terminfo dir");
        fs::write(
            bundle_dir.join("bin").join("taskers"),
            "#!/bin/sh\nexit 0\n",
        )
        .expect("taskers");
        fs::write(
            bundle_dir.join("bin").join("taskersctl"),
            "#!/bin/sh\nexit 0\n",
        )
        .expect("taskersctl");
        fs::write(
            bundle_dir.join("ghostty").join(".taskers-runtime-version"),
            env!("CARGO_PKG_VERSION"),
        )
        .expect("ghostty version");
        fs::write(
            bundle_dir
                .join("ghostty")
                .join("lib")
                .join("libtaskers_ghostty_bridge.so"),
            "bridge",
        )
        .expect("bridge");
        fs::write(
            bundle_dir
                .join("ghostty")
                .join("lib")
                .join("libghostty_gtk.so"),
            "bridge",
        )
        .expect("generic bridge");
        fs::write(
            bundle_dir
                .join("ghostty")
                .join("themes")
                .join("Catppuccin Mocha"),
            "palette = 0=#1e1e2e\n",
        )
        .expect("theme");
        fs::write(
            bundle_dir.join("terminfo").join("g").join("ghostty"),
            "ghostty",
        )
        .expect("terminfo");

        let archive_path = temp.path().join("taskers-linux-bundle.tar.xz");
        {
            let file = fs::File::create(&archive_path).expect("archive");
            let encoder = XzEncoder::new(file, 6);
            let mut tar = Builder::new(encoder);
            tar.append_dir_all(".", &bundle_dir).expect("append");
            tar.into_inner().expect("encoder").finish().expect("finish");
        }

        let checksum = sha256_path(&archive_path).expect("sha");
        let target = current_target_triple().expect("target");
        let manifest_path = temp.path().join("manifest.json");
        let manifest = ReleaseManifest {
            version: env!("CARGO_PKG_VERSION").to_string(),
            artifacts: BTreeMap::from([(
                target.to_string(),
                ReleaseArtifact {
                    kind: ArtifactKind::LinuxBundleV1,
                    url: archive_path.display().to_string(),
                    sha256: checksum,
                    size_bytes: None,
                },
            )]),
        };
        fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(&manifest).expect("manifest json"),
        )
        .expect("manifest write");

        unsafe {
            std::env::set_var("TASKERS_INSTALL_ROOT", &install_root);
            std::env::set_var("TASKERS_RELEASE_MANIFEST_URL", &manifest_path);
        }
        let installation =
            ManagedInstallation::ensure_installed(env!("CARGO_PKG_VERSION")).expect("install");
        unsafe {
            std::env::remove_var("TASKERS_INSTALL_ROOT");
            std::env::remove_var("TASKERS_RELEASE_MANIFEST_URL");
        }

        assert!(installation.executable_path().is_file());
        assert!(installation.taskersctl_path().is_file());
        assert!(installation.bundle_root.join("bin").join("taskers-terminald").is_file());
        assert!(installation.ghostty_resources_path().is_dir());
        assert!(installation.terminfo_path().is_dir());
        assert!(
            installation
                .bundle_root
                .join("ghostty")
                .join("lib")
                .join("libghostty_gtk.so")
                .is_file()
        );
    }

    #[test]
    fn prefers_path_taskers_entry_when_it_matches_current_exe() {
        let temp = tempdir().expect("tempdir");
        let install_bin = temp.path().join("xdg-bin");
        let real_bin = temp.path().join("cargo-bin");
        fs::create_dir_all(&install_bin).expect("install bin");
        fs::create_dir_all(&real_bin).expect("real bin");

        let current_exe = real_bin.join("taskers");
        fs::write(&current_exe, "#!/bin/sh\n").expect("current exe");
        #[cfg(unix)]
        symlink(&current_exe, install_bin.join("taskers")).expect("taskers symlink");

        let path_env = OsString::from(install_bin.as_os_str());
        let resolved = path_taskers_executable(&current_exe, Some(path_env)).expect("path taskers");

        assert_eq!(resolved, install_bin.join("taskers"));
    }

    #[test]
    fn repo_local_binaries_do_not_look_installed() {
        let repo_binary = PathBuf::from("/home/notes/Projects/taskers/target/debug/taskers");
        assert!(!launcher_path_looks_installed(&repo_binary));
    }

    #[test]
    fn preserves_non_launcher_desktop_entry() {
        let temp = tempdir().expect("tempdir");
        let desktop_entry = temp.path().join("dev.taskers.app.desktop");
        fs::write(
            &desktop_entry,
            "[Desktop Entry]\nExec=/home/notes/.cargo/bin/taskers-gtk\n",
        )
        .expect("desktop entry");

        let launcher = PathBuf::from("/home/notes/.local/bin/taskers");
        assert!(
            !should_update_desktop_entry(&desktop_entry, &launcher, &launcher, None)
                .expect("decision"),
        );
    }

    #[test]
    fn updates_matching_launcher_desktop_entry() {
        let temp = tempdir().expect("tempdir");
        let desktop_entry = temp.path().join("dev.taskers.app.desktop");
        let launcher = PathBuf::from("/home/notes/.local/bin/taskers-desktop-launch");
        fs::write(
            &desktop_entry,
            format!("[Desktop Entry]\nExec={}\n", desktop_exec(&launcher)),
        )
        .expect("desktop entry");

        assert!(
            should_update_desktop_entry(&desktop_entry, &launcher, &launcher, None)
                .expect("decision")
        );
    }

    #[test]
    fn updates_legacy_dev_desktop_entry() {
        let temp = tempdir().expect("tempdir");
        let desktop_entry = temp.path().join("dev.taskers.app.desktop");
        let desktop_launcher = PathBuf::from("/home/notes/.local/bin/taskers-desktop-launch");
        let launcher = PathBuf::from("/home/notes/.local/bin/taskers");
        let legacy_launcher = PathBuf::from("/home/notes/.cargo/bin/taskers-gtk-desktop-launch");
        fs::write(
            &desktop_entry,
            format!("[Desktop Entry]\nExec={}\n", desktop_exec(&legacy_launcher)),
        )
        .expect("desktop entry");

        assert!(
            should_update_desktop_entry(
                &desktop_entry,
                &desktop_launcher,
                &launcher,
                Some(&legacy_launcher),
            )
            .expect("decision")
        );
    }

    #[test]
    fn removes_legacy_desktop_wrapper() {
        let temp = tempdir().expect("tempdir");
        let cargo_bin_home = temp.path().join("cargo-bin");
        fs::create_dir_all(&cargo_bin_home).expect("cargo bin dir");

        let legacy_wrapper = cargo_bin_home.join("taskers-gtk-desktop-launch");
        fs::write(&legacy_wrapper, "#!/bin/sh\n").expect("legacy wrapper");

        assert!(remove_legacy_desktop_integration(Some(&legacy_wrapper)).expect("cleanup"),);
        assert!(!legacy_wrapper.exists());
    }

    #[test]
    fn desktop_launch_wrapper_targets_binary_with_log_redirection() {
        let contents = desktop_launch_wrapper_contents(Path::new("/home/notes/.local/bin/taskers"));
        assert!(contents.contains("desktop-launch-diagnostics.log"));
        assert!(contents.contains("desktop-launch.log"));
        assert!(contents.contains("/usr/bin/setsid -f"));
        assert!(contents.contains("focus_taskers_window_niri_once"));
        assert!(!contents.contains("focus_taskers_window_niri_retry"));
        assert!(contents.contains("niri msg action focus-window --id"));
        assert!(contents.contains("if focus_taskers_window_niri_once; then\n  exit 0\nfi"));
        assert!(!contents.contains("break"));
        assert!(contents.contains("'/home/notes/.local/bin/taskers'"));
    }
}
