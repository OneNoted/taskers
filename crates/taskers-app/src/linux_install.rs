use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result, anyhow};

const SKIP_DESKTOP_INTEGRATION_ENV: &str = "TASKERS_SKIP_DESKTOP_INTEGRATION";
const RELEASE_EXECUTABLE_NAMES: &[&str] = &["taskers", "taskers-gtk"];

pub fn run(args: &[OsString]) -> Result<ExitStatus> {
    install_linux_user_assets()?;

    let current_exe = env::current_exe().context("failed to resolve current taskers path")?;
    let executable = sibling_binary(&current_exe, "taskers-gtk")?;
    let taskersctl = sibling_binary(&current_exe, "taskersctl")?;

    let mut command = Command::new(&executable);
    command.args(args);
    command.env("TASKERS_CTL_PATH", taskersctl);

    command
        .status()
        .with_context(|| format!("failed to launch {}", executable.display()))
}

pub fn exit_code_from_status(status: ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        status.signal().map_or(1, |signal| 128 + signal)
    }

    #[cfg(not(unix))]
    {
        1
    }
}

pub fn install_linux_user_assets() -> Result<()> {
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
    let launcher_release_execs =
        launcher_managed_release_execs(&taskers_paths::default_release_install_root());
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
        &launcher_release_execs,
    )? {
        fs::write(&desktop_entry_path, desktop_entry)
            .with_context(|| format!("failed to write {}", desktop_entry_path.display()))?;
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

fn sibling_binary(current_exe: &Path, binary_name: &str) -> Result<PathBuf> {
    let resolved = fs::canonicalize(current_exe)
        .with_context(|| format!("failed to resolve {}", current_exe.display()))?;
    let parent = resolved
        .parent()
        .ok_or_else(|| anyhow!("{} has no parent directory", resolved.display()))?;
    let candidate = parent.join(binary_name);
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(anyhow!(
            "expected sibling binary {} next to {}",
            candidate.display(),
            resolved.display()
        ))
    }
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
        "#!/bin/sh\nset -eu\nlog_dir=\"${{XDG_CACHE_HOME:-$HOME/.cache}}/taskers\"\nmkdir -p \"$log_dir\"\n\nfocus_taskers_window_niri_once() {{\n  command -v niri >/dev/null 2>&1 || return 1\n  command -v jq >/dev/null 2>&1 || return 1\n\n  window_id=\"$(niri msg -j windows 2>/dev/null | jq -r 'first(.[] | select(.app_id == \"dev.taskers.app\") | .id) // empty' 2>/dev/null)\"\n  [ -n \"$window_id\" ] || return 1\n  niri msg action focus-window --id \"$window_id\" >/dev/null 2>&1\n}}\n\nfocus_taskers_window_niri_retry() {{\n  command -v niri >/dev/null 2>&1 || return 1\n  command -v jq >/dev/null 2>&1 || return 1\n\n  (\n    i=0\n    while [ \"$i\" -lt 40 ]; do\n      sleep 0.1\n      focus_taskers_window_niri_once >/dev/null 2>&1 || true\n      i=$((i + 1))\n    done\n  ) >/dev/null 2>&1 &\n}}\n\nif focus_taskers_window_niri_once; then\n  focus_taskers_window_niri_retry || true\n  exit 0\nfi\n\n/usr/bin/setsid -f {target} --diagnostic-log \"$log_dir/desktop-launch-diagnostics.log\" >>\"$log_dir/desktop-launch.log\" 2>&1\nfocus_taskers_window_niri_retry || true\n",
        target = shell_single_quote(target),
    )
}

fn should_update_desktop_entry(
    path: &Path,
    desktop_launcher: &Path,
    launcher: &Path,
    legacy_desktop_launcher: Option<&Path>,
    launcher_release_execs: &[String],
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
    managed_execs.extend(launcher_release_execs.iter().cloned());

    Ok(managed_execs
        .iter()
        .any(|candidate| candidate == existing_exec))
}

fn launcher_managed_release_execs(release_root: &Path) -> Vec<String> {
    let mut execs = Vec::new();
    let Ok(versions) = fs::read_dir(release_root) else {
        return execs;
    };

    for version in versions.flatten() {
        let Ok(targets) = fs::read_dir(version.path()) else {
            continue;
        };

        for target in targets.flatten() {
            let bin_dir = target.path().join("bin");
            for executable_name in RELEASE_EXECUTABLE_NAMES {
                let executable = bin_dir.join(executable_name);
                if executable.is_file() {
                    execs.push(desktop_exec(&executable));
                }
            }
        }
    }

    execs
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

#[cfg(test)]
mod tests {
    use super::{
        desktop_exec, desktop_launch_wrapper_contents, exit_code_from_status,
        launcher_managed_release_execs, launcher_path_looks_installed, path_taskers_executable,
        remove_legacy_desktop_integration, should_update_desktop_entry,
    };
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{
        ffi::OsString,
        fs,
        path::{Path, PathBuf},
        process::Command,
    };
    use tempfile::tempdir;

    #[test]
    fn preserves_normal_exit_codes() {
        let status = Command::new("sh")
            .args(["-c", "exit 7"])
            .status()
            .expect("spawn shell");
        assert_eq!(exit_code_from_status(status), 7);
    }

    #[cfg(unix)]
    #[test]
    fn maps_signals_to_failure_exit_codes() {
        let status = Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .status()
            .expect("spawn shell");
        assert_eq!(exit_code_from_status(status), 143);
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
            !should_update_desktop_entry(&desktop_entry, &launcher, &launcher, None, &[])
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
            should_update_desktop_entry(&desktop_entry, &launcher, &launcher, None, &[])
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
                &[],
            )
            .expect("decision")
        );
    }

    #[test]
    fn updates_launcher_managed_release_desktop_entry() {
        let temp = tempdir().expect("tempdir");
        let desktop_entry = temp.path().join("dev.taskers.app.desktop");
        let desktop_launcher = PathBuf::from("/home/notes/.local/bin/taskers-desktop-launch");
        let launcher = PathBuf::from("/home/notes/.cargo/bin/taskers");
        let release_root = temp.path().join("releases");
        let release_exec = release_root
            .join("0.5.0")
            .join("x86_64-unknown-linux-gnu")
            .join("bin")
            .join("taskers-gtk");
        fs::create_dir_all(release_exec.parent().expect("bin dir")).expect("create release dir");
        fs::write(&release_exec, "#!/bin/sh\n").expect("release executable");
        fs::write(
            &desktop_entry,
            format!("[Desktop Entry]\nExec={}\n", desktop_exec(&release_exec)),
        )
        .expect("desktop entry");

        let release_execs = launcher_managed_release_execs(&release_root);
        assert!(
            should_update_desktop_entry(
                &desktop_entry,
                &desktop_launcher,
                &launcher,
                None,
                &release_execs,
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

        assert!(remove_legacy_desktop_integration(Some(&legacy_wrapper)).expect("cleanup"));
        assert!(!legacy_wrapper.exists());
    }

    #[test]
    fn desktop_launch_wrapper_targets_binary_with_log_redirection() {
        let contents = desktop_launch_wrapper_contents(Path::new("/home/notes/.cargo/bin/taskers"));
        assert!(contents.contains("desktop-launch-diagnostics.log"));
        assert!(contents.contains("desktop-launch.log"));
        assert!(contents.contains("/usr/bin/setsid -f"));
        assert!(contents.contains("focus_taskers_window_niri_once"));
        assert!(contents.contains("focus_taskers_window_niri_retry"));
        assert!(contents.contains("niri msg action focus-window --id"));
        assert!(contents.contains("'/home/notes/.cargo/bin/taskers'"));
    }
}
