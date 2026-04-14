use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
#[cfg(unix)]
use libc::{self, passwd};
#[cfg(unix)]
use std::{ffi::CStr, os::unix::ffi::OsStringExt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellKind {
    Bash,
    Fish,
    Zsh,
    Other,
}

const INHERITED_TERMINAL_ENV_KEYS: &[&str] = &[
    "TERM",
    "TERMINFO",
    "TERMINFO_DIRS",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "COLORTERM",
    "NO_COLOR",
    "CLICOLOR",
    "CLICOLOR_FORCE",
    "KITTY_INSTALLATION_DIR",
    "KITTY_LISTEN_ON",
    "KITTY_PUBLIC_KEY",
    "KITTY_WINDOW_ID",
    "GHOSTTY_BIN_DIR",
    "GHOSTTY_RESOURCES_DIR",
    "GHOSTTY_SHELL_FEATURES",
    "GHOSTTY_SHELL_INTEGRATION_XDG_DIR",
];

#[derive(Debug, Clone)]
pub struct ShellIntegration {
    root: PathBuf,
    wrapper_path: PathBuf,
    real_shell: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellLaunchSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

impl ShellLaunchSpec {
    pub fn fallback() -> Self {
        let program = default_shell_program();
        let args = match shell_kind(&program) {
            ShellKind::Fish => vec!["--interactive".into()],
            ShellKind::Bash | ShellKind::Zsh => vec!["-i".into()],
            ShellKind::Other => Vec::new(),
        };
        Self {
            program,
            args,
            env: base_env(),
        }
    }

    pub fn program_and_args(&self) -> Vec<String> {
        let mut argv = Vec::with_capacity(self.args.len() + 1);
        argv.push(self.program.display().to_string());
        argv.extend(self.args.iter().cloned());
        argv
    }
}

impl ShellIntegration {
    pub fn install(configured_shell: Option<&str>) -> Result<Self> {
        let root = runtime_root();
        let wrapper_path = root.join("taskers-shell-wrapper.sh");
        let real_shell = resolve_shell_program(configured_shell)?;

        install_runtime_assets(&root)?;
        install_agent_shims(&root)?;

        Ok(Self {
            root,
            wrapper_path,
            real_shell,
        })
    }

    pub fn launch_spec(&self) -> ShellLaunchSpec {
        let profile = std::env::var("TASKERS_SHELL_PROFILE").unwrap_or_else(|_| "default".into());
        let integration_disabled = std::env::var_os("TASKERS_DISABLE_SHELL_INTEGRATION").is_some();

        match shell_kind(&self.real_shell) {
            ShellKind::Bash if !integration_disabled => {
                let mut env = self.base_env();
                env.insert(
                    "TASKERS_REAL_SHELL".into(),
                    self.real_shell.display().to_string(),
                );
                env.insert("TASKERS_SHELL_PROFILE".into(), profile);
                if let Some(value) = std::env::var_os("TASKERS_USER_BASHRC") {
                    env.insert(
                        "TASKERS_USER_BASHRC".into(),
                        value.to_string_lossy().into_owned(),
                    );
                }

                ShellLaunchSpec {
                    program: self.wrapper_path.clone(),
                    args: Vec::new(),
                    env,
                }
            }
            ShellKind::Bash => ShellLaunchSpec {
                program: self.real_shell.clone(),
                args: vec!["--noprofile".into(), "--norc".into(), "-i".into()],
                env: self.base_env(),
            },
            ShellKind::Fish if !integration_disabled => {
                let mut env = self.base_env();
                env.insert(
                    "TASKERS_REAL_SHELL".into(),
                    self.real_shell.display().to_string(),
                );
                env.insert("TASKERS_SHELL_PROFILE".into(), profile.clone());

                let mut args = Vec::new();
                if profile == "clean" {
                    args.push("--no-config".into());
                }
                args.push("--interactive".into());
                args.push("--init-command".into());
                args.push(fish_source_command());

                ShellLaunchSpec {
                    program: self.wrapper_path.clone(),
                    args,
                    env,
                }
            }
            ShellKind::Fish => ShellLaunchSpec {
                program: self.real_shell.clone(),
                args: vec!["--no-config".into(), "--interactive".into()],
                env: self.base_env(),
            },
            ShellKind::Zsh if !integration_disabled => {
                let mut env = self.base_env();
                env.insert(
                    "TASKERS_REAL_SHELL".into(),
                    self.real_shell.display().to_string(),
                );
                env.insert("TASKERS_SHELL_PROFILE".into(), profile.clone());
                env.insert(
                    "ZDOTDIR".into(),
                    zsh_runtime_dir(&self.root).display().to_string(),
                );
                if let Some(value) = env::var_os("ZDOTDIR").or_else(|| env::var_os("HOME")) {
                    env.insert(
                        "TASKERS_USER_ZDOTDIR".into(),
                        value.to_string_lossy().into_owned(),
                    );
                }
                let args = if profile == "clean" {
                    vec!["-d".into(), "-i".into()]
                } else {
                    vec!["-i".into()]
                };

                ShellLaunchSpec {
                    program: self.wrapper_path.clone(),
                    args,
                    env,
                }
            }
            ShellKind::Zsh => ShellLaunchSpec {
                program: self.real_shell.clone(),
                args: vec!["-d".into(), "-f".into(), "-i".into()],
                env: self.base_env(),
            },
            ShellKind::Other => {
                let mut env = self.base_env();
                env.insert(
                    "TASKERS_REAL_SHELL".into(),
                    self.real_shell.display().to_string(),
                );
                ShellLaunchSpec {
                    program: self.wrapper_path.clone(),
                    args: Vec::new(),
                    env,
                }
            }
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl ShellIntegration {
    fn base_env(&self) -> BTreeMap<String, String> {
        let mut env = base_env();
        env.insert(
            "TASKERS_SHELL_INTEGRATION_DIR".into(),
            self.root.display().to_string(),
        );
        if let Some(path) = resolve_taskersctl_path() {
            env.insert("TASKERS_CTL_PATH".into(), path.display().to_string());
        }
        let shim_dir = self.root.join("bin");
        env.insert("PATH".into(), prepend_path_entry(&shim_dir));
        env
    }
}

pub fn install_shell_integration(configured_shell: Option<&str>) -> Result<ShellIntegration> {
    ShellIntegration::install(configured_shell)
}

pub fn scrub_inherited_terminal_env() {
    for key in INHERITED_TERMINAL_ENV_KEYS {
        unsafe {
            env::remove_var(key);
        }
    }
}

pub fn default_shell_program() -> PathBuf {
    login_shell_from_passwd()
        .or_else(shell_from_env)
        .unwrap_or_else(|| PathBuf::from("/bin/sh"))
}

pub fn validate_shell_program(configured_shell: Option<&str>) -> Result<Option<PathBuf>> {
    configured_shell
        .and_then(normalize_shell_override)
        .map(|value| resolve_shell_override(&value))
        .transpose()
}

fn base_env() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("TASKERS_EMBEDDED".into(), "1".into());
    env.insert("TERM_PROGRAM".into(), "taskers".into());
    env
}

fn install_agent_shims(root: &Path) -> Result<()> {
    let shim_dir = root.join("bin");
    fs::create_dir_all(&shim_dir)
        .with_context(|| format!("failed to create {}", shim_dir.display()))?;
    for (name, target_path) in [
        ("codex", root.join("taskers-agent-codex.sh")),
        ("claude", root.join("taskers-agent-claude.sh")),
        ("claude-code", root.join("taskers-agent-claude.sh")),
        ("opencode", root.join("taskers-agent-proxy.sh")),
        ("aider", root.join("taskers-agent-proxy.sh")),
    ] {
        let shim_path = shim_dir.join(name);
        if shim_path.symlink_metadata().is_ok() {
            fs::remove_file(&shim_path)
                .with_context(|| format!("failed to replace {}", shim_path.display()))?;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target_path, &shim_path).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                shim_path.display(),
                target_path.display()
            )
        })?;

        #[cfg(not(unix))]
        fs::copy(&target_path, &shim_path).with_context(|| {
            format!(
                "failed to copy {} -> {}",
                target_path.display(),
                shim_path.display()
            )
        })?;
    }

    Ok(())
}

fn prepend_path_entry(entry: &Path) -> String {
    let mut parts = vec![entry.display().to_string()];
    if let Some(path) = env::var_os("PATH") {
        parts.extend(
            env::split_paths(&path)
                .filter(|candidate| candidate != entry)
                .map(|candidate| candidate.display().to_string()),
        );
    }
    parts.join(":")
}

fn runtime_root() -> PathBuf {
    taskers_paths::default_shell_runtime_dir()
}

fn write_asset(path: &Path, content: &str, executable: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))?;

    #[cfg(unix)]
    if executable {
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

fn resolve_taskersctl_path() -> Option<PathBuf> {
    if let Some(path) = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.join("taskersctl")))
        .filter(|path| path.is_file())
    {
        return Some(path);
    }

    if let Some(path) = env::var_os("TASKERS_CTL_PATH")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        return Some(path);
    }

    if let Some(home) = env::var_os("HOME").map(PathBuf::from) {
        for candidate in [
            home.join(".cargo").join("bin").join("taskersctl"),
            home.join(".local").join("bin").join("taskersctl"),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|entry| entry.join("taskersctl"))
        .find(|candidate| candidate.is_file())
}

fn resolve_shell_program(configured_shell: Option<&str>) -> Result<PathBuf> {
    if let Some(shell) = configured_shell.and_then(|value| normalize_shell_override(value)) {
        return resolve_shell_override(&shell)
            .with_context(|| format!("failed to resolve configured shell {shell}"));
    }

    Ok(default_shell_program())
}

fn shell_kind(path: &Path) -> ShellKind {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .trim_start_matches('-');

    match name {
        "bash" => ShellKind::Bash,
        "fish" => ShellKind::Fish,
        "zsh" => ShellKind::Zsh,
        _ => ShellKind::Other,
    }
}

fn normalize_shell_override(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn resolve_shell_override(value: &str) -> Result<PathBuf> {
    let expanded = expand_home_prefix(value);
    let candidate = PathBuf::from(&expanded);
    if expanded.contains('/') {
        anyhow::ensure!(
            candidate.is_file(),
            "shell program {} does not exist",
            candidate.display()
        );
        return Ok(candidate);
    }

    let path_var = env::var_os("PATH").unwrap_or_default();
    let resolved = env::split_paths(&path_var)
        .map(|entry| entry.join(&candidate))
        .find(|entry| entry.is_file());
    resolved.with_context(|| format!("shell program {value} was not found in PATH"))
}

fn expand_home_prefix(value: &str) -> String {
    if value == "~" {
        return env::var("HOME").unwrap_or_else(|_| value.to_string());
    }

    if let Some(suffix) = value.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(suffix).display().to_string();
        }
    }

    value.to_string()
}

fn shell_from_env() -> Option<PathBuf> {
    env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

#[cfg(unix)]
fn login_shell_from_passwd() -> Option<PathBuf> {
    let uid = unsafe { libc::geteuid() };
    let mut pwd = std::mem::MaybeUninit::<passwd>::uninit();
    let mut result = std::ptr::null_mut::<passwd>();
    let mut buffer = vec![0u8; passwd_buffer_size()];

    let status = unsafe {
        libc::getpwuid_r(
            uid,
            pwd.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if status != 0 || result.is_null() {
        return None;
    }

    let pwd = unsafe { pwd.assume_init() };
    if pwd.pw_shell.is_null() {
        return None;
    }

    let shell = unsafe { CStr::from_ptr(pwd.pw_shell) }.to_bytes().to_vec();
    if shell.is_empty() {
        return None;
    }

    Some(PathBuf::from(std::ffi::OsString::from_vec(shell)))
}

#[cfg(not(unix))]
fn login_shell_from_passwd() -> Option<PathBuf> {
    None
}

#[cfg(unix)]
fn passwd_buffer_size() -> usize {
    let size = unsafe { libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX) };
    if size <= 0 { 4096 } else { size as usize }
}

#[cfg(not(unix))]
fn passwd_buffer_size() -> usize {
    4096
}

fn fish_source_command() -> String {
    r#"source "$TASKERS_SHELL_INTEGRATION_DIR/taskers-hooks.fish""#.into()
}

fn zsh_runtime_dir(root: &Path) -> PathBuf {
    root.join("zsh")
}

fn install_runtime_assets(root: &Path) -> Result<()> {
    write_asset(
        &root.join("taskers-shell-wrapper.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-shell-wrapper.sh"
        )),
        true,
    )?;
    write_asset(
        &root.join("bash").join("taskers.bashrc"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/bash/taskers.bashrc"
        )),
        false,
    )?;
    write_asset(
        &root.join("taskers-hooks.bash"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        )),
        false,
    )?;
    write_asset(
        &root.join("taskers-hooks.fish"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        )),
        false,
    )?;
    write_asset(
        &root.join("taskers-hooks.zsh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        )),
        false,
    )?;
    write_asset(
        &zsh_runtime_dir(root).join(".zshenv"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/zsh/.zshenv"
        )),
        false,
    )?;
    write_asset(
        &zsh_runtime_dir(root).join(".zshrc"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/zsh/.zshrc"
        )),
        false,
    )?;
    write_asset(
        &zsh_runtime_dir(root).join(".zcompdump"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/zsh/.zcompdump"
        )),
        false,
    )?;
    write_asset(
        &root.join("taskers-codex-notify.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-codex-notify.sh"
        )),
        true,
    )?;
    write_asset(
        &root.join("taskers-claude-hook.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-claude-hook.sh"
        )),
        true,
    )?;
    write_asset(
        &root.join("taskers-agent-codex.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-codex.sh"
        )),
        true,
    )?;
    write_asset(
        &root.join("taskers-agent-claude.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-claude.sh"
        )),
        true,
    )?;
    write_asset(
        &root.join("taskers-agent-proxy.sh"),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-proxy.sh"
        )),
        true,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::Mutex,
        time::{Duration, SystemTime},
    };

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::{
        INHERITED_TERMINAL_ENV_KEYS, ShellIntegration, expand_home_prefix, fish_source_command,
        install_runtime_assets, normalize_shell_override, resolve_shell_override, zsh_runtime_dir,
    };
    use crate::{CommandSpec, PtySession};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn shell_override_normalizes_blank_values() {
        assert_eq!(normalize_shell_override(""), None);
        assert_eq!(normalize_shell_override("   "), None);
        assert_eq!(
            normalize_shell_override(" /usr/bin/fish "),
            Some("/usr/bin/fish".into())
        );
    }

    #[test]
    fn fish_source_command_uses_runtime_env_path() {
        assert_eq!(
            fish_source_command(),
            r#"source "$TASKERS_SHELL_INTEGRATION_DIR/taskers-hooks.fish""#
        );
    }

    #[test]
    fn zsh_launch_spec_routes_through_runtime_zdotdir() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let original_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", "/tmp/taskers-home");
            std::env::set_var("ZDOTDIR", "/tmp/user-zdotdir");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
        }

        let integration = ShellIntegration {
            root: PathBuf::from("/tmp/taskers-runtime"),
            wrapper_path: PathBuf::from("/tmp/taskers-runtime/taskers-shell-wrapper.sh"),
            real_shell: PathBuf::from("/usr/bin/zsh"),
        };
        let spec = integration.launch_spec();

        assert_eq!(
            spec.env.get("ZDOTDIR").map(String::as_str),
            Some("/tmp/taskers-runtime/zsh")
        );
        assert_eq!(
            spec.env.get("TASKERS_USER_ZDOTDIR").map(String::as_str),
            Some("/tmp/user-zdotdir")
        );
        assert_eq!(spec.program, integration.wrapper_path);
        assert_eq!(spec.args, vec!["-i"]);

        unsafe {
            match original_zdotdir {
                Some(value) => std::env::set_var("ZDOTDIR", value),
                None => std::env::remove_var("ZDOTDIR"),
            }
            match original_home {
                Some(value) => std::env::set_var("HOME", value),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn zsh_runtime_dir_is_nested_under_runtime_root() {
        assert_eq!(
            zsh_runtime_dir(&PathBuf::from("/tmp/taskers-runtime")),
            PathBuf::from("/tmp/taskers-runtime/zsh")
        );
    }

    #[test]
    fn install_runtime_assets_writes_zsh_runtime_files() {
        let root = unique_temp_dir("taskers-runtime-test");
        install_runtime_assets(&root).expect("install runtime assets");

        assert!(root.join("taskers-shell-wrapper.sh").is_file());
        assert!(root.join("taskers-hooks.bash").is_file());
        assert!(root.join("taskers-hooks.fish").is_file());
        assert!(root.join("taskers-hooks.zsh").is_file());
        assert!(root.join("taskers-codex-notify.sh").is_file());
        assert!(root.join("taskers-claude-hook.sh").is_file());
        assert!(root.join("taskers-agent-codex.sh").is_file());
        assert!(root.join("taskers-agent-claude.sh").is_file());
        assert!(root.join("taskers-agent-proxy.sh").is_file());
        assert!(zsh_runtime_dir(&root).join(".zshenv").is_file());
        assert!(zsh_runtime_dir(&root).join(".zshrc").is_file());
        assert!(zsh_runtime_dir(&root).join(".zcompdump").is_file());

        fs::remove_dir_all(&root).expect("cleanup runtime assets");
    }

    #[test]
    fn home_prefix_expansion_without_home_keeps_original_shape() {
        let original = "~/bin/fish";
        let expanded = expand_home_prefix(original);
        if std::env::var_os("HOME").is_some() {
            assert_ne!(expanded, original);
        } else {
            assert_eq!(expanded, original);
        }
    }

    #[test]
    fn inherited_terminal_env_keys_cover_color_and_terminfo_leaks() {
        for key in ["NO_COLOR", "TERMINFO", "TERMINFO_DIRS", "TERM_PROGRAM"] {
            assert!(
                INHERITED_TERMINAL_ENV_KEYS.contains(&key),
                "expected {key} to be scrubbed from inherited terminal env"
            );
        }
    }

    #[test]
    fn shell_wrapper_exports_taskers_tty_name() {
        let wrapper = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-shell-wrapper.sh"
        ));
        assert!(
            wrapper.contains("TASKERS_TTY_NAME"),
            "expected wrapper to export TASKERS_TTY_NAME"
        );
    }

    #[test]
    fn shell_wrapper_routes_terminal_sessions_through_sidecar_attach() {
        let wrapper = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-shell-wrapper.sh"
        ));
        assert!(
            wrapper.contains("TASKERS_TERMINAL_SOCKET"),
            "expected wrapper to branch on terminal socket availability"
        );
        assert!(
            wrapper.contains("TASKERS_TERMINAL_SESSION_ID"),
            "expected wrapper to require terminal session ids for session attach"
        );
        assert!(
            wrapper.contains("session attach"),
            "expected wrapper to delegate continuity startup to taskersctl session attach"
        );
    }

    #[test]
    fn shell_wrapper_handles_fish_and_zsh_default_launch_modes() {
        let wrapper = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-shell-wrapper.sh"
        ));
        assert!(
            wrapper.contains("SHELL_PROFILE=${TASKERS_SHELL_PROFILE:-default}"),
            "expected wrapper to honor TASKERS_SHELL_PROFILE when synthesizing default shell args"
        );
        assert!(
            wrapper.contains("--init-command"),
            "expected wrapper to synthesize fish init-command integration when no explicit args are passed"
        );
        assert!(
            wrapper.contains("set -- -d -i"),
            "expected wrapper to synthesize zsh default launch flags when no explicit args are passed"
        );
    }

    #[test]
    fn fish_and_zsh_launch_specs_preserve_shell_profile_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let original_profile = std::env::var_os("TASKERS_SHELL_PROFILE");
        let original_disabled = std::env::var_os("TASKERS_DISABLE_SHELL_INTEGRATION");
        unsafe {
            std::env::set_var("TASKERS_SHELL_PROFILE", "clean");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
        }

        let fish_integration = ShellIntegration {
            root: PathBuf::from("/tmp/taskers-runtime"),
            wrapper_path: PathBuf::from("/tmp/taskers-runtime/taskers-shell-wrapper.sh"),
            real_shell: PathBuf::from("/usr/bin/fish"),
        };
        let zsh_integration = ShellIntegration {
            root: PathBuf::from("/tmp/taskers-runtime"),
            wrapper_path: PathBuf::from("/tmp/taskers-runtime/taskers-shell-wrapper.sh"),
            real_shell: PathBuf::from("/usr/bin/zsh"),
        };

        let fish_spec = fish_integration.launch_spec();
        let zsh_spec = zsh_integration.launch_spec();

        assert_eq!(
            fish_spec
                .env
                .get("TASKERS_SHELL_PROFILE")
                .map(String::as_str),
            Some("clean")
        );
        assert_eq!(
            zsh_spec
                .env
                .get("TASKERS_SHELL_PROFILE")
                .map(String::as_str),
            Some("clean")
        );

        unsafe {
            match original_profile {
                Some(value) => std::env::set_var("TASKERS_SHELL_PROFILE", value),
                None => std::env::remove_var("TASKERS_SHELL_PROFILE"),
            }
            match original_disabled {
                Some(value) => std::env::set_var("TASKERS_DISABLE_SHELL_INTEGRATION", value),
                None => std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION"),
            }
        }
    }

    #[test]
    fn shell_hooks_and_proxy_require_surface_tty_identity() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));
        let agent_proxy = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-proxy.sh"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                asset.contains("TASKERS_SURFACE_ID"),
                "expected asset to require TASKERS_SURFACE_ID"
            );
            assert!(
                asset.contains("TASKERS_TTY_NAME"),
                "expected asset to require TASKERS_TTY_NAME"
            );
        }
        assert!(
            agent_proxy.contains("TASKERS_AGENT_PROXY_ACTIVE"),
            "expected proxy asset to keep loop-prevention guard"
        );
    }

    #[test]
    fn shell_hooks_only_treat_agent_identity_as_live_process_state() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                !asset.contains("TASKERS_PANE_AGENT_KIND"),
                "expected hook asset to avoid sticky pane-level agent identity"
            );
        }
    }

    #[test]
    fn shell_assets_do_not_auto_report_completed_on_clean_or_interrupted_exit() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));
        let agent_proxy = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-proxy.sh"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                !asset.contains("taskers__emit_with_metadata completed"),
                "expected hook asset to avoid auto-emitting completed on bare agent exit"
            );
        }
        assert!(
            !agent_proxy.contains("emit_signal completed"),
            "expected proxy to avoid auto-emitting completed on bare agent exit"
        );
        assert!(
            !agent_proxy.contains("emit_signal error"),
            "expected proxy to avoid owning stop/error signaling"
        );
    }

    #[test]
    fn zsh_shell_hook_avoids_readonly_status_parameter_name() {
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));

        assert!(
            !zsh_hooks.contains("local status="),
            "expected zsh hooks to avoid assigning to zsh's readonly status parameter"
        );
    }

    #[test]
    fn zsh_shell_hook_tracks_directory_changes_for_metadata() {
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));

        assert!(
            zsh_hooks.contains("add-zsh-hook chpwd taskers__on_chpwd")
                || zsh_hooks.contains("chpwd_functions+=(taskers__on_chpwd)"),
            "expected zsh hooks to refresh metadata on directory changes"
        );
    }

    #[test]
    fn zsh_shell_hook_prefers_shell_tty_and_supports_jj_repos() {
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));

        assert!(
            zsh_hooks.contains("local current_tty=${TTY:-}"),
            "expected zsh hooks to prefer zsh's built-in TTY variable"
        );
        assert!(
            zsh_hooks.contains("jj root"),
            "expected zsh hooks to support JJ repo root detection"
        );
    }

    #[test]
    fn shell_hooks_emit_boolean_agent_active_flags() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                asset.contains("true"),
                "expected hook asset to emit literal boolean true"
            );
            assert!(
                asset.contains("false"),
                "expected hook asset to emit literal boolean false"
            );
        }
    }

    #[test]
    fn embedded_zsh_emits_metadata_for_repo_cwd() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers-runtime-zsh-metadata");
        install_runtime_assets(&runtime_root).expect("install runtime assets");

        let home_dir = runtime_root.join("home");
        let real_bin_dir = runtime_root.join("real-bin");
        let repo_dir = runtime_root.join("repo");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");
        fs::create_dir_all(&repo_dir).expect("repo dir");

        let taskersctl_path = runtime_root.join("taskersctl");
        let test_log = runtime_root.join("taskersctl.log");
        write_executable(
            &taskersctl_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKERS_TEST_LOG\"\n",
        );
        write_executable(
            &real_bin_dir.join("git"),
            "#!/bin/sh\ncwd=\nif [ \"$1\" = \"-C\" ]; then cwd=$2; shift 2; fi\nif [ \"$1\" = \"rev-parse\" ] && [ \"$2\" = \"--show-toplevel\" ]; then printf '%s\\n' \"$cwd\"; exit 0; fi\nif [ \"$1\" = \"symbolic-ref\" ] && [ \"$2\" = \"--quiet\" ] && [ \"$3\" = \"--short\" ] && [ \"$4\" = \"HEAD\" ]; then printf 'main\\n'; exit 0; fi\nif [ \"$1\" = \"rev-parse\" ] && [ \"$2\" = \"--short\" ] && [ \"$3\" = \"HEAD\" ]; then printf 'abc123\\n'; exit 0; fi\nexit 1\n",
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let zsh_path = resolve_shell_override("zsh").expect("resolve zsh");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("ZDOTDIR");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            );
        }

        let integration = ShellIntegration {
            root: runtime_root.clone(),
            wrapper_path: runtime_root.join("taskers-shell-wrapper.sh"),
            real_shell: zsh_path,
        };
        let mut launch = integration.launch_spec();
        launch.env.insert(
            "TASKERS_CTL_PATH".into(),
            taskersctl_path.display().to_string(),
        );
        launch
            .env
            .insert("TASKERS_WORKSPACE_ID".into(), "ws".into());
        launch.env.insert("TASKERS_PANE_ID".into(), "pn".into());
        launch.env.insert("TASKERS_SURFACE_ID".into(), "sf".into());
        launch
            .env
            .insert("TASKERS_TEST_LOG".into(), test_log.display().to_string());

        let mut spec = CommandSpec::new(launch.program.display().to_string());
        spec.args = launch.args;
        spec.env = launch.env;
        spec.cwd = Some(repo_dir.clone());

        let mut spawned = PtySession::spawn(&spec).expect("spawn shell");
        std::thread::sleep(Duration::from_millis(250));
        spawned.session.write_all(b"exit\n").expect("exit shell");

        let mut reader = spawned.reader;
        let mut buffer = [0u8; 1024];
        while reader.read_into(&mut buffer).unwrap_or(0) > 0 {}

        let log = fs::read_to_string(&test_log).expect("read metadata log");
        assert!(
            log.contains("signal --source shell --kind metadata"),
            "expected zsh shell to emit metadata, got: {log}"
        );
        assert!(
            log.contains(&format!("--cwd {}", repo_dir.display())),
            "expected metadata cwd in log, got: {log}"
        );
        assert!(
            log.contains("--repo repo"),
            "expected repo name in log, got: {log}"
        );
        assert!(
            log.contains("--branch main"),
            "expected git branch in log, got: {log}"
        );

        restore_env_var("HOME", original_home);
        restore_env_var("PATH", original_path);
        restore_env_var("ZDOTDIR", original_zdotdir);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    #[test]
    fn embedded_zsh_falls_back_to_jj_branch_when_git_probe_fails() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers-runtime-zsh-jj-fallback");
        install_runtime_assets(&runtime_root).expect("install runtime assets");

        let home_dir = runtime_root.join("home");
        let real_bin_dir = runtime_root.join("real-bin");
        let repo_dir = runtime_root.join("repo");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");
        fs::create_dir_all(&repo_dir).expect("repo dir");

        let taskersctl_path = runtime_root.join("taskersctl");
        let test_log = runtime_root.join("taskersctl.log");
        write_executable(
            &taskersctl_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKERS_TEST_LOG\"\n",
        );
        write_executable(&real_bin_dir.join("git"), "#!/bin/sh\nexit 1\n");
        write_executable(
            &real_bin_dir.join("jj"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" = \"root\" ]; then printf '%s\\n' \"{}\"; exit 0; fi\nif [ \"$1\" = \"log\" ]; then printf 'jj123456\\n'; exit 0; fi\nexit 1\n",
                repo_dir.display()
            ),
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let zsh_path = resolve_shell_override("zsh").expect("resolve zsh");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("ZDOTDIR");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            );
        }

        let integration = ShellIntegration {
            root: runtime_root.clone(),
            wrapper_path: runtime_root.join("taskers-shell-wrapper.sh"),
            real_shell: zsh_path,
        };
        let mut launch = integration.launch_spec();
        launch.env.insert(
            "TASKERS_CTL_PATH".into(),
            taskersctl_path.display().to_string(),
        );
        launch
            .env
            .insert("TASKERS_WORKSPACE_ID".into(), "ws".into());
        launch.env.insert("TASKERS_PANE_ID".into(), "pn".into());
        launch.env.insert("TASKERS_SURFACE_ID".into(), "sf".into());
        launch
            .env
            .insert("TASKERS_TEST_LOG".into(), test_log.display().to_string());

        let mut spec = CommandSpec::new(launch.program.display().to_string());
        spec.args = launch.args;
        spec.env = launch.env;
        spec.cwd = Some(repo_dir.clone());

        let mut spawned = PtySession::spawn(&spec).expect("spawn shell");
        std::thread::sleep(Duration::from_millis(250));
        spawned.session.write_all(b"exit\n").expect("exit shell");

        let mut reader = spawned.reader;
        let mut buffer = [0u8; 1024];
        while reader.read_into(&mut buffer).unwrap_or(0) > 0 {}

        let log = fs::read_to_string(&test_log).expect("read metadata log");
        assert!(
            log.contains("signal --source shell --kind metadata"),
            "expected zsh shell to emit metadata, got: {log}"
        );
        assert!(
            log.contains(&format!("--cwd {}", repo_dir.display())),
            "expected metadata cwd in log, got: {log}"
        );
        assert!(
            log.contains("--repo repo"),
            "expected repo name in log, got: {log}"
        );
        assert!(
            log.contains("--branch jj123456"),
            "expected JJ branch fallback in log, got: {log}"
        );

        restore_env_var("HOME", original_home);
        restore_env_var("PATH", original_path);
        restore_env_var("ZDOTDIR", original_zdotdir);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    #[test]
    fn bash_shell_hook_marks_prompt_only_once_until_preexec_runs() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));

        assert!(
            bash_hooks.contains("TASKERS_OSC133_PROMPT_MARKED"),
            "expected bash hooks to track whether the prompt is already OSC133-marked"
        );
        assert!(
            bash_hooks.contains("TASKERS_OSC133_SAVE_PS1:-"),
            "expected bash hooks to treat saved prompt copies as part of the marked state"
        );
        assert!(
            bash_hooks.contains("TASKERS_OSC133_PROMPT_MARKED=1"),
            "expected bash hooks to keep the marked state synchronized with the prompt save guards"
        );
    }

    #[test]
    fn shell_hooks_invalidate_metadata_cache_after_agent_exit() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                asset.contains("TASKERS_LAST_META_AGENT"),
                "expected hook asset to invalidate cached agent metadata after exit"
            );
            assert!(
                asset.contains("TASKERS_LAST_META_AGENT_ACTIVE"),
                "expected hook asset to invalidate cached agent-active metadata after exit"
            );
        }
    }

    #[test]
    fn agent_proxy_owns_explicit_surface_agent_lifecycle_commands() {
        let bash_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.bash"
        ));
        let zsh_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.zsh"
        ));
        let fish_hooks = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-hooks.fish"
        ));
        let agent_proxy = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/shell/taskers-agent-proxy.sh"
        ));

        for asset in [bash_hooks, zsh_hooks, fish_hooks] {
            assert!(
                !asset.contains("surface agent-start"),
                "expected hook asset to leave explicit lifecycle start to the proxy"
            );
            assert!(
                !asset.contains("surface agent-stop"),
                "expected hook asset to leave explicit lifecycle stop to the proxy"
            );
        }
        assert!(
            agent_proxy.contains("surface agent-start"),
            "expected proxy asset to emit explicit surface agent start commands"
        );
        assert!(
            agent_proxy.contains("surface agent-stop"),
            "expected proxy asset to emit explicit surface agent stop commands"
        );
    }

    #[test]
    fn embedded_zsh_codex_command_emits_surface_lifecycle_via_proxy() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers-runtime-proxy-clean");
        install_runtime_assets(&runtime_root).expect("install runtime assets");
        super::install_agent_shims(&runtime_root).expect("install agent shims");

        let home_dir = runtime_root.join("home");
        let real_bin_dir = runtime_root.join("real-bin");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");

        let taskersctl_path = runtime_root.join("taskersctl");
        let args_log = runtime_root.join("codex-args.log");
        write_executable(
            &taskersctl_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKERS_TEST_LOG\"\n",
        );
        write_executable(
            &real_bin_dir.join("codex"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_CODEX_ARGS_LOG\"\nnotify_script=\nprev=\nfor arg in \"$@\"; do\n  if [ \"$prev\" = \"-c\" ]; then\n    notify_script=$(printf '%s' \"$arg\" | sed -n 's/^notify=\\[\"bash\",\"\\([^\"]*\\)\"\\]$/\\1/p')\n    prev=\n    continue\n  fi\n  prev=$arg\ndone\nif [ -n \"$notify_script\" ]; then\n  \"$notify_script\" '{\"last-assistant-message\":\"Turn complete\"}'\nfi\nprintf 'fake codex\\n'\nexit 0\n",
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let zsh_path = resolve_shell_override("zsh").expect("resolve zsh");
        let test_log = runtime_root.join("taskersctl.log");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("ZDOTDIR");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            );
        }

        let integration = ShellIntegration {
            root: runtime_root.clone(),
            wrapper_path: runtime_root.join("taskers-shell-wrapper.sh"),
            real_shell: zsh_path,
        };
        let mut launch = integration.launch_spec();
        launch.args.push("-c".into());
        launch.args.push("codex".into());
        launch.env.insert(
            "TASKERS_CTL_PATH".into(),
            taskersctl_path.display().to_string(),
        );
        launch
            .env
            .insert("TASKERS_WORKSPACE_ID".into(), "ws".into());
        launch.env.insert("TASKERS_PANE_ID".into(), "pn".into());
        launch.env.insert("TASKERS_SURFACE_ID".into(), "sf".into());
        launch
            .env
            .insert("TASKERS_TEST_LOG".into(), test_log.display().to_string());
        launch
            .env
            .insert("FAKE_CODEX_ARGS_LOG".into(), args_log.display().to_string());

        let mut spec = CommandSpec::new(launch.program.display().to_string());
        spec.args = launch.args;
        spec.env = launch.env;

        let spawned = PtySession::spawn(&spec).expect("spawn shell");
        let mut reader = spawned.reader;
        let mut buffer = [0u8; 1024];
        let mut output = String::new();
        loop {
            let bytes_read = reader.read_into(&mut buffer).unwrap_or(0);
            if bytes_read == 0 {
                break;
            }
            output.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));
        }

        let log = fs::read_to_string(&test_log)
            .unwrap_or_else(|error| panic!("read lifecycle log failed: {error}; output={output}"));
        let codex_args = fs::read_to_string(&args_log)
            .unwrap_or_else(|error| panic!("read codex args log failed: {error}; output={output}"));
        assert!(
            codex_args.contains("-c\n") || codex_args.contains("-c "),
            "expected codex wrapper to inject config override, got: {codex_args}"
        );
        assert!(
            codex_args.contains("notify=[\"bash\",\""),
            "expected codex wrapper to inject notify helper override, got: {codex_args}"
        );
        assert!(
            log.contains("surface agent-start --workspace ws --pane pn --surface sf --agent codex"),
            "expected start lifecycle in log, got: {log}"
        );
        assert!(
            log.contains("agent-hook stop --workspace ws --pane pn --surface sf --agent codex --title Codex --message Turn complete"),
            "expected codex notify helper to emit stop hook, got: {log}"
        );
        assert!(
            log.contains(
                "surface agent-stop --workspace ws --pane pn --surface sf --exit-status 0"
            ),
            "expected stop lifecycle in log, got: {log}"
        );

        restore_env_var("HOME", original_home);
        restore_env_var("PATH", original_path);
        restore_env_var("ZDOTDIR", original_zdotdir);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    #[test]
    fn embedded_zsh_claude_command_injects_taskers_hooks_and_process_lifecycle() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers-runtime-proxy-claude");
        install_runtime_assets(&runtime_root).expect("install runtime assets");
        super::install_agent_shims(&runtime_root).expect("install agent shims");

        let home_dir = runtime_root.join("home");
        let real_bin_dir = runtime_root.join("real-bin");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");

        let taskersctl_path = runtime_root.join("taskersctl");
        let args_log = runtime_root.join("claude-args.log");
        write_executable(
            &taskersctl_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKERS_TEST_LOG\"\n",
        );
        write_executable(
            &real_bin_dir.join("claude"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FAKE_CLAUDE_ARGS_LOG\"\nexit 0\n",
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let zsh_path = resolve_shell_override("zsh").expect("resolve zsh");
        let test_log = runtime_root.join("taskersctl.log");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("ZDOTDIR");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            );
        }

        let integration = ShellIntegration {
            root: runtime_root.clone(),
            wrapper_path: runtime_root.join("taskers-shell-wrapper.sh"),
            real_shell: zsh_path,
        };
        let mut launch = integration.launch_spec();
        launch.args.push("-c".into());
        launch.args.push("claude --help".into());
        launch.env.insert(
            "TASKERS_CTL_PATH".into(),
            taskersctl_path.display().to_string(),
        );
        launch
            .env
            .insert("TASKERS_WORKSPACE_ID".into(), "ws".into());
        launch.env.insert("TASKERS_PANE_ID".into(), "pn".into());
        launch.env.insert("TASKERS_SURFACE_ID".into(), "sf".into());
        launch
            .env
            .insert("TASKERS_TEST_LOG".into(), test_log.display().to_string());
        launch.env.insert(
            "FAKE_CLAUDE_ARGS_LOG".into(),
            args_log.display().to_string(),
        );

        let mut spec = CommandSpec::new(launch.program.display().to_string());
        spec.args = launch.args;
        spec.env = launch.env;

        let spawned = PtySession::spawn(&spec).expect("spawn shell");
        let mut reader = spawned.reader;
        let mut buffer = [0u8; 1024];
        let mut output = String::new();
        loop {
            let bytes_read = reader.read_into(&mut buffer).unwrap_or(0);
            if bytes_read == 0 {
                break;
            }
            output.push_str(&String::from_utf8_lossy(&buffer[..bytes_read]));
        }

        let log = fs::read_to_string(&test_log)
            .unwrap_or_else(|error| panic!("read lifecycle log failed: {error}; output={output}"));
        let claude_args = fs::read_to_string(&args_log).unwrap_or_else(|error| {
            panic!("read claude args log failed: {error}; output={output}")
        });
        let hook_path = runtime_root.join("taskers-claude-hook.sh");
        assert!(
            claude_args.contains("--settings"),
            "expected claude wrapper to inject hook settings, got: {claude_args}"
        );
        assert!(
            claude_args.contains(&hook_path.display().to_string())
                && claude_args.contains("user-prompt-submit"),
            "expected claude wrapper to inject prompt-submit hook path, got: {claude_args}"
        );
        assert!(
            claude_args.contains(&hook_path.display().to_string()) && claude_args.contains("stop"),
            "expected claude wrapper to inject stop hook path, got: {claude_args}"
        );
        assert!(
            log.contains(
                "surface agent-start --workspace ws --pane pn --surface sf --agent claude"
            ),
            "expected start lifecycle in log, got: {log}"
        );
        assert!(
            log.contains(
                "surface agent-stop --workspace ws --pane pn --surface sf --exit-status 0"
            ),
            "expected stop lifecycle in log, got: {log}"
        );

        restore_env_var("HOME", original_home);
        restore_env_var("PATH", original_path);
        restore_env_var("ZDOTDIR", original_zdotdir);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    #[test]
    fn claude_code_shim_preserves_binary_lookup_and_quotes_hook_paths() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers runtime claude code");
        install_runtime_assets(&runtime_root).expect("install runtime assets");
        super::install_agent_shims(&runtime_root).expect("install agent shims");

        let real_bin_dir = runtime_root.join("real-bin");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");

        let capture_path = runtime_root.join("claude-code-capture.log");
        write_executable(
            &real_bin_dir.join("claude-code"),
            "#!/bin/sh\nprintf 'target=%s\\n' \"${TASKERS_AGENT_PROXY_TARGET:-}\" >> \"$FAKE_CLAUDE_CAPTURE\"\nprintf '%s\\n' \"$@\" >> \"$FAKE_CLAUDE_CAPTURE\"\nexit 0\n",
        );

        let original_path = std::env::var_os("PATH");
        let shim_path = runtime_root.join("bin").join("claude-code");
        let output = Command::new(&shim_path)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            )
            .env("FAKE_CLAUDE_CAPTURE", &capture_path)
            .arg("--help")
            .output()
            .expect("run claude-code shim");

        assert!(
            output.status.success(),
            "expected claude-code shim to succeed, stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );

        let capture = fs::read_to_string(&capture_path).expect("read capture log");
        let hook_path = runtime_root.join("taskers-claude-hook.sh");
        assert!(
            capture.contains("target=claude-code"),
            "expected shim to preserve the invoked claude-code lookup target, got: {capture}"
        );
        assert!(
            capture.contains("--settings"),
            "expected claude-code shim to forward hook settings, got: {capture}"
        );
        assert!(
            capture.contains(&format!("'{}' user-prompt-submit", hook_path.display())),
            "expected claude-code hook path to be single-quoted inside settings, got: {capture}"
        );

        restore_env_var("PATH", original_path);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    #[test]
    fn embedded_zsh_ctrl_c_reports_interrupted_surface_stop_via_proxy() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|poison| poison.into_inner());
        let runtime_root = unique_temp_dir("taskers-runtime-proxy-interrupt");
        install_runtime_assets(&runtime_root).expect("install runtime assets");
        super::install_agent_shims(&runtime_root).expect("install agent shims");

        let home_dir = runtime_root.join("home");
        let real_bin_dir = runtime_root.join("real-bin");
        fs::create_dir_all(&home_dir).expect("home dir");
        fs::create_dir_all(&real_bin_dir).expect("real bin dir");

        let taskersctl_path = runtime_root.join("taskersctl");
        write_executable(
            &taskersctl_path,
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$TASKERS_TEST_LOG\"\n",
        );
        write_executable(
            &real_bin_dir.join("codex"),
            "#!/bin/sh\ntrap 'exit 130' INT\nwhile :; do sleep 1; done\n",
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_zdotdir = std::env::var_os("ZDOTDIR");
        let zsh_path = resolve_shell_override("zsh").expect("resolve zsh");
        let test_log = runtime_root.join("taskersctl.log");
        unsafe {
            std::env::set_var("HOME", &home_dir);
            std::env::remove_var("ZDOTDIR");
            std::env::remove_var("TASKERS_DISABLE_SHELL_INTEGRATION");
            std::env::remove_var("TASKERS_SHELL_PROFILE");
            std::env::set_var(
                "PATH",
                format!(
                    "{}:{}",
                    real_bin_dir.display(),
                    original_path
                        .as_deref()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ),
            );
        }

        let integration = ShellIntegration {
            root: runtime_root.clone(),
            wrapper_path: runtime_root.join("taskers-shell-wrapper.sh"),
            real_shell: zsh_path,
        };
        let mut launch = integration.launch_spec();
        launch.args.push("-c".into());
        launch.args.push("codex".into());
        launch.env.insert(
            "TASKERS_CTL_PATH".into(),
            taskersctl_path.display().to_string(),
        );
        launch
            .env
            .insert("TASKERS_WORKSPACE_ID".into(), "ws".into());
        launch.env.insert("TASKERS_PANE_ID".into(), "pn".into());
        launch.env.insert("TASKERS_SURFACE_ID".into(), "sf".into());
        launch
            .env
            .insert("TASKERS_TEST_LOG".into(), test_log.display().to_string());

        let mut spec = CommandSpec::new(launch.program.display().to_string());
        spec.args = launch.args;
        spec.env = launch.env;

        let mut spawned = PtySession::spawn(&spec).expect("spawn shell");
        std::thread::sleep(Duration::from_millis(250));
        spawned
            .session
            .write_all(b"\x03")
            .expect("send ctrl-c to shell");

        let mut reader = spawned.reader;
        let mut buffer = [0u8; 1024];
        while reader.read_into(&mut buffer).unwrap_or(0) > 0 {}

        let log = fs::read_to_string(&test_log).expect("read lifecycle log");
        assert!(
            log.contains("surface agent-start --workspace ws --pane pn --surface sf --agent codex"),
            "expected start lifecycle in log, got: {log}"
        );
        assert!(
            log.contains(
                "surface agent-stop --workspace ws --pane pn --surface sf --exit-status 130"
            ),
            "expected interrupted stop lifecycle in log, got: {log}"
        );

        restore_env_var("HOME", original_home);
        restore_env_var("PATH", original_path);
        restore_env_var("ZDOTDIR", original_zdotdir);
        fs::remove_dir_all(&runtime_root).expect("cleanup runtime root");
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{unique}"))
    }

    fn restore_env_var(key: &str, value: Option<std::ffi::OsString>) {
        unsafe {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    fn write_executable(path: &PathBuf, content: &str) {
        fs::write(path, content).expect("write script");
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(path, permissions).expect("chmod script");
        }
    }
}
