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
use taskers_paths::{
    HostPlatform, default_macos_applications_link_path, default_release_install_root,
};
use xz2::read::XzDecoder;
use zip::ZipArchive;

const INSTALL_ROOT_ENV: &str = "TASKERS_INSTALL_ROOT";
const MANIFEST_URL_ENV: &str = "TASKERS_RELEASE_MANIFEST_URL";
const SKIP_CODESIGN_VERIFY_ENV: &str = "TASKERS_SKIP_CODESIGN_VERIFY";

pub fn run() -> Result<ExitStatus> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let installation = ManagedInstallation::ensure_installed(env!("CARGO_PKG_VERSION"))?;

    if installation.platform == HostPlatform::Linux {
        installation.install_linux_user_assets()?;
    }

    installation.launch(&args)
}

#[derive(Debug, Deserialize, Serialize)]
struct ReleaseManifest {
    version: String,
    artifacts: BTreeMap<String, ReleaseArtifact>,
    #[serde(default)]
    manual_downloads: BTreeMap<String, ManualDownload>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ReleaseArtifact {
    kind: ArtifactKind,
    url: String,
    sha256: String,
    #[serde(default)]
    size_bytes: Option<u64>,
    #[serde(default)]
    minimum_os_version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct ManualDownload {
    url: String,
    sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    #[serde(rename = "linux_bundle_v1")]
    LinuxBundleV1,
    #[serde(rename = "macos_app_zip_v1")]
    MacosAppZipV1,
}

#[derive(Debug)]
struct ManagedInstallation {
    platform: HostPlatform,
    target_triple: String,
    version: String,
    bundle_root: PathBuf,
}

impl ManagedInstallation {
    fn ensure_installed(version: &str) -> Result<Self> {
        let platform = HostPlatform::detect();
        let target_triple = current_target_triple(platform)?;
        let install_root = env::var_os(INSTALL_ROOT_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(default_release_install_root);
        let bundle_root = bundle_root(&install_root, version, &target_triple, platform);
        let installation = Self {
            platform,
            target_triple: target_triple.to_string(),
            version: version.to_string(),
            bundle_root,
        };

        if installation.is_complete() {
            installation.install_platform_integrations()?;
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
        installation.install_artifact(&manifest, artifact)?;
        installation.install_platform_integrations()?;
        Ok(installation)
    }

    fn launch(&self, args: &[OsString]) -> Result<ExitStatus> {
        let executable = self.executable_path();
        let mut command = Command::new(&executable);
        command.args(args);

        if self.platform == HostPlatform::Linux {
            command.env("TASKERS_CTL_PATH", self.taskersctl_path());
            command.env("TASKERS_GHOSTTY_RUNTIME_DIR", self.ghostty_resources_path());
            command.env("GHOSTTY_RESOURCES_DIR", self.ghostty_resources_path());
            command.env("TERMINFO", self.terminfo_path());
            command.env("TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP", "1");
        }

        command
            .status()
            .with_context(|| format!("failed to launch {}", executable.display()))
    }

    fn load_manifest(&self) -> Result<ReleaseManifest> {
        let manifest_url = env::var(MANIFEST_URL_ENV)
            .unwrap_or_else(|_| default_manifest_url(&self.version));
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
        let _ = &manifest.manual_downloads;
        Ok(manifest)
    }

    fn install_artifact(
        &self,
        _manifest: &ReleaseManifest,
        artifact: &ReleaseArtifact,
    ) -> Result<()> {
        validate_artifact_kind(self.platform, artifact.kind)?;

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
        let staging_root = version_root.join(format!(".install-{}-{timestamp}", std::process::id()));
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

        match artifact.kind {
            ArtifactKind::LinuxBundleV1 => unpack_linux_bundle(&download_path, &unpack_root)?,
            ArtifactKind::MacosAppZipV1 => unpack_macos_zip(&download_path, &unpack_root)?,
        }

        let staged_bundle_root = unpack_root;
        if !validate_bundle_layout(self.platform, &staged_bundle_root) {
            bail!(
                "artifact {} did not unpack the expected layout into {}",
                artifact.url,
                staged_bundle_root.display()
            );
        }

        if self.platform == HostPlatform::Macos {
            verify_codesign(&staged_bundle_root.join("Taskers.app"))?;
        }

        if self.bundle_root.exists() {
            remove_path(&self.bundle_root)?;
        }
        if let Some(parent) = self.bundle_root.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::rename(&staged_bundle_root, &self.bundle_root).with_context(|| {
            format!(
                "failed to move {} to {}",
                staged_bundle_root.display(),
                self.bundle_root.display()
            )
        })?;
        remove_path(&staging_root)?;

        Ok(())
    }

    fn install_platform_integrations(&self) -> Result<()> {
        if self.platform == HostPlatform::Macos {
            self.refresh_macos_app_link()?;
        }
        Ok(())
    }

    fn install_linux_user_assets(&self) -> Result<()> {
        let launcher = env::current_exe().context("failed to resolve current launcher path")?;
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

        let desktop_entry = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/taskers.desktop.in"
        ))
        .replace("{{EXEC}}", &desktop_exec(&launcher));
        fs::write(
            applications_dir.join("dev.taskers.app.desktop"),
            desktop_entry,
        )
        .with_context(|| format!("failed to write {}", applications_dir.display()))?;
        fs::write(
            icons_dir.join("taskers.svg"),
            include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/taskers.svg")),
        )
        .with_context(|| format!("failed to write {}", icons_dir.display()))?;

        let notify_path = xdg_bin_home.join("taskers-codex-notify");
        write_executable(
            &notify_path,
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/taskers-codex-notify.sh"
            )),
        )?;

        refresh_desktop_indexes(&applications_dir);
        Ok(())
    }

    fn refresh_macos_app_link(&self) -> Result<()> {
        let Some(link_path) = default_macos_applications_link_path() else {
            return Ok(());
        };

        let target = self.bundle_root.join("Taskers.app");
        if let Some(parent) = link_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if let Ok(metadata) = fs::symlink_metadata(&link_path) {
            if metadata.file_type().is_symlink() {
                fs::remove_file(&link_path)
                    .with_context(|| format!("failed to remove {}", link_path.display()))?;
            } else {
                return Ok(());
            }
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link_path).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                link_path.display(),
                target.display()
            )
        })?;

        Ok(())
    }

    fn is_complete(&self) -> bool {
        validate_bundle_layout(self.platform, &self.bundle_root)
    }

    fn executable_path(&self) -> PathBuf {
        match self.platform {
            HostPlatform::Linux => self.bundle_root.join("bin").join("taskers"),
            HostPlatform::Macos => self
                .bundle_root
                .join("Taskers.app")
                .join("Contents")
                .join("MacOS")
                .join("Taskers"),
            HostPlatform::Other => self.bundle_root.join("taskers"),
        }
    }

    fn taskersctl_path(&self) -> PathBuf {
        match self.platform {
            HostPlatform::Linux => self.bundle_root.join("bin").join("taskersctl"),
            HostPlatform::Macos => self
                .bundle_root
                .join("Taskers.app")
                .join("Contents")
                .join("Resources")
                .join("bin")
                .join("taskersctl"),
            HostPlatform::Other => self.bundle_root.join("taskersctl"),
        }
    }

    fn ghostty_resources_path(&self) -> PathBuf {
        match self.platform {
            HostPlatform::Linux => self.bundle_root.join("ghostty"),
            HostPlatform::Macos => self
                .bundle_root
                .join("Taskers.app")
                .join("Contents")
                .join("Resources")
                .join("ghostty"),
            HostPlatform::Other => self.bundle_root.join("ghostty"),
        }
    }

    fn terminfo_path(&self) -> PathBuf {
        match self.platform {
            HostPlatform::Linux => self.bundle_root.join("terminfo"),
            HostPlatform::Macos => self
                .bundle_root
                .join("Taskers.app")
                .join("Contents")
                .join("Resources")
                .join("terminfo"),
            HostPlatform::Other => self.bundle_root.join("terminfo"),
        }
    }
}

fn validate_artifact_kind(platform: HostPlatform, kind: ArtifactKind) -> Result<()> {
    match (platform, kind) {
        (HostPlatform::Linux, ArtifactKind::LinuxBundleV1)
        | (HostPlatform::Macos, ArtifactKind::MacosAppZipV1) => Ok(()),
        _ => bail!("artifact kind {kind:?} is incompatible with platform {platform:?}"),
    }
}

fn validate_bundle_layout(platform: HostPlatform, bundle_root: &Path) -> bool {
    match platform {
        HostPlatform::Linux => {
            bundle_root.join("bin").join("taskers").is_file()
                && bundle_root.join("bin").join("taskersctl").is_file()
                && bundle_root.join("ghostty").is_dir()
                && bundle_root
                    .join("ghostty")
                    .join("lib")
                    .join("libtaskers_ghostty_bridge.so")
                    .is_file()
                && bundle_root.join("terminfo").is_dir()
        }
        HostPlatform::Macos => {
            bundle_root
                .join("Taskers.app")
                .join("Contents")
                .join("MacOS")
                .join("Taskers")
                .is_file()
                && bundle_root
                    .join("Taskers.app")
                    .join("Contents")
                    .join("Resources")
                    .join("bin")
                    .join("taskersctl")
                    .is_file()
                && bundle_root
                    .join("Taskers.app")
                    .join("Contents")
                    .join("Resources")
                    .join("ghostty")
                    .is_dir()
                && bundle_root
                    .join("Taskers.app")
                    .join("Contents")
                    .join("Resources")
                    .join("terminfo")
                    .is_dir()
        }
        HostPlatform::Other => false,
    }
}

fn current_target_triple(platform: HostPlatform) -> Result<&'static str> {
    match (platform, env::consts::ARCH) {
        (HostPlatform::Linux, "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        (HostPlatform::Macos, "aarch64") => Ok("aarch64-apple-darwin"),
        (HostPlatform::Macos, "x86_64") => Ok("x86_64-apple-darwin"),
        _ => bail!(
            "unsupported platform/architecture combination: {:?}/{}",
            platform,
            env::consts::ARCH
        ),
    }
}

fn bundle_root(
    install_root: &Path,
    version: &str,
    target_triple: &str,
    platform: HostPlatform,
) -> PathBuf {
    match platform {
        HostPlatform::Linux => install_root.join(version).join(target_triple),
        HostPlatform::Macos => install_root.join(version),
        HostPlatform::Other => install_root.join(version).join(target_triple),
    }
}

fn default_manifest_url(version: &str) -> String {
    let repository = env!("CARGO_PKG_REPOSITORY").trim_end_matches('/');
    format!(
        "{repository}/releases/download/v{version}/taskers-manifest-v{version}.json"
    )
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
            format!("failed to copy {} to {}", path.display(), destination.display())
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
    value
        .strip_prefix("file://")
        .map(|path| PathBuf::from(path))
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
    let file =
        fs::File::open(archive_path).with_context(|| format!("failed to open {}", archive_path.display()))?;
    let decoder = XzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination)
        .with_context(|| format!("failed to unpack {}", archive_path.display()))
}

fn unpack_macos_zip(zip_path: &Path, destination: &Path) -> Result<()> {
    let file =
        fs::File::open(zip_path).with_context(|| format!("failed to open {}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file)
        .with_context(|| format!("failed to decode {}", zip_path.display()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .with_context(|| format!("failed to open zip entry {index}"))?;
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| anyhow!("zip entry {} escaped the destination", entry.name()))?
            .to_path_buf();
        let output_path = destination.join(relative);

        if entry.name().ends_with('/') {
            fs::create_dir_all(&output_path)
                .with_context(|| format!("failed to create {}", output_path.display()))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        let mut output = fs::File::create(&output_path)
            .with_context(|| format!("failed to create {}", output_path.display()))?;
        io::copy(&mut entry, &mut output)
            .with_context(|| format!("failed to write {}", output_path.display()))?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;

            fs::set_permissions(&output_path, fs::Permissions::from_mode(mode)).with_context(
                || format!("failed to chmod {}", output_path.display()),
            )?;
        }
    }

    Ok(())
}

fn verify_codesign(app_path: &Path) -> Result<()> {
    if env::var_os(SKIP_CODESIGN_VERIFY_ENV).is_some() {
        return Ok(());
    }

    let status = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app_path)
        .status()
        .with_context(|| format!("failed to invoke codesign for {}", app_path.display()))?;
    if !status.success() {
        bail!("codesign verification failed for {}", app_path.display());
    }
    Ok(())
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
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove {}", path.display()))?;
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
        current_target_triple, default_manifest_url, sha256_path,
    };
    use std::{collections::BTreeMap, fs, path::PathBuf};
    use tar::Builder;
    use taskers_paths::HostPlatform;
    use tempfile::tempdir;
    use xz2::write::XzEncoder;

    #[test]
    fn default_manifest_url_uses_exact_version() {
        let url = default_manifest_url("0.2.1");
        assert!(url.ends_with("/releases/download/v0.2.1/taskers-manifest-v0.2.1.json"));
    }

    #[test]
    fn bundle_roots_match_platform_layout() {
        let root = PathBuf::from("/tmp/taskers");
        assert_eq!(
            bundle_root(
                &root,
                "0.2.1",
                "x86_64-unknown-linux-gnu",
                HostPlatform::Linux
            ),
            PathBuf::from("/tmp/taskers/0.2.1/x86_64-unknown-linux-gnu")
        );
        assert_eq!(
            bundle_root(&root, "0.2.1", "aarch64-apple-darwin", HostPlatform::Macos),
            PathBuf::from("/tmp/taskers/0.2.1")
        );
    }

    #[test]
    fn linux_bundle_install_from_local_manifest() {
        let temp = tempdir().expect("tempdir");
        let install_root = temp.path().join("install");
        let bundle_dir = temp.path().join("bundle");
        fs::create_dir_all(bundle_dir.join("bin")).expect("bin dir");
        fs::create_dir_all(bundle_dir.join("ghostty").join("lib")).expect("ghostty dir");
        fs::create_dir_all(bundle_dir.join("terminfo").join("g")).expect("terminfo dir");
        fs::write(bundle_dir.join("bin").join("taskers"), "#!/bin/sh\nexit 0\n").expect("taskers");
        fs::write(bundle_dir.join("bin").join("taskersctl"), "#!/bin/sh\nexit 0\n")
            .expect("taskersctl");
        fs::write(bundle_dir.join("ghostty").join(".taskers-runtime-version"), "0.2.1")
            .expect("ghostty version");
        fs::write(
            bundle_dir
                .join("ghostty")
                .join("lib")
                .join("libtaskers_ghostty_bridge.so"),
            "bridge",
        )
        .expect("bridge");
        fs::write(bundle_dir.join("terminfo").join("g").join("ghostty"), "ghostty")
            .expect("terminfo");

        let archive_path = temp.path().join("taskers-linux-bundle.tar.xz");
        {
            let file = fs::File::create(&archive_path).expect("archive");
            let encoder = XzEncoder::new(file, 6);
            let mut tar = Builder::new(encoder);
            tar.append_dir_all(".", &bundle_dir).expect("append");
            tar.into_inner()
                .expect("encoder")
                .finish()
                .expect("finish");
        }

        let checksum = sha256_path(&archive_path).expect("sha");
        let target = current_target_triple(HostPlatform::Linux).expect("target");
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
                    minimum_os_version: None,
                },
            )]),
            manual_downloads: BTreeMap::new(),
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
        assert!(installation.ghostty_resources_path().is_dir());
        assert!(installation.terminfo_path().is_dir());
    }
}
