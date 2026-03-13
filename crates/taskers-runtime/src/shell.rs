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

        write_asset(
            &wrapper_path,
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
            &root.join("taskers-shell-bridge.py"),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/shell/taskers-shell-bridge.py"
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
                let env = self.base_env();

                let mut args = Vec::new();
                if profile == "clean" {
                    args.push("--no-config".into());
                }
                args.push("--interactive".into());
                args.push("--init-command".into());
                args.push(fish_source_command());

                ShellLaunchSpec {
                    program: self.real_shell.clone(),
                    args,
                    env,
                }
            }
            ShellKind::Fish => ShellLaunchSpec {
                program: self.real_shell.clone(),
                args: vec!["--no-config".into(), "--interactive".into()],
                env: self.base_env(),
            },
            ShellKind::Zsh => {
                let args = if profile == "clean" || integration_disabled {
                    vec!["-d".into(), "-f".into(), "-i".into()]
                } else {
                    vec!["-i".into()]
                };

                ShellLaunchSpec {
                    program: self.real_shell.clone(),
                    args,
                    env: self.base_env(),
                }
            }
            ShellKind::Other => ShellLaunchSpec {
                program: self.real_shell.clone(),
                args: Vec::new(),
                env: self.base_env(),
            },
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
        env.insert(
            "TASKERS_SHELL_BRIDGE_PATH".into(),
            self.root
                .join("taskers-shell-bridge.py")
                .display()
                .to_string(),
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
    let proxy_path = root.join("taskers-agent-proxy.sh");

    for name in ["codex", "claude", "claude-code", "opencode", "aider"] {
        let shim_path = shim_dir.join(name);
        if shim_path.symlink_metadata().is_ok() {
            fs::remove_file(&shim_path)
                .with_context(|| format!("failed to replace {}", shim_path.display()))?;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&proxy_path, &shim_path).with_context(|| {
            format!(
                "failed to symlink {} -> {}",
                shim_path.display(),
                proxy_path.display()
            )
        })?;

        #[cfg(not(unix))]
        fs::copy(&proxy_path, &shim_path).with_context(|| {
            format!(
                "failed to copy {} -> {}",
                proxy_path.display(),
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
    if let Some(path) = std::env::var_os("TASKERS_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return path.join("shell");
    }

    if let Some(path) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return path.join("taskers").join("shell");
    }

    std::env::temp_dir().join("taskers-runtime").join("shell")
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

#[cfg(test)]
mod tests {
    use super::{expand_home_prefix, fish_source_command, normalize_shell_override};

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
    fn home_prefix_expansion_without_home_keeps_original_shape() {
        let original = "~/bin/fish";
        let expanded = expand_home_prefix(original);
        if std::env::var_os("HOME").is_some() {
            assert_ne!(expanded, original);
        } else {
            assert_eq!(expanded, original);
        }
    }
}
