use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use taskers_paths::default_tmux_socket_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxBackend {
    program: PathBuf,
    socket_path: PathBuf,
}

impl TmuxBackend {
    pub fn detect() -> Result<Self, String> {
        Self::detect_with_socket(default_tmux_socket_path())
    }

    pub fn detect_with_socket(socket_path: PathBuf) -> Result<Self, String> {
        let program = PathBuf::from("tmux");
        match Command::new(&program).arg("-V").output() {
            Ok(output) if output.status.success() => Ok(Self {
                program,
                socket_path,
            }),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                if stderr.is_empty() {
                    Err(format!("tmux probe exited with status {}", output.status))
                } else {
                    Err(format!("tmux probe failed: {stderr}"))
                }
            }
            Err(error) => Err(format!("tmux unavailable: {error}")),
        }
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn session_name(&self, session_id: &str) -> String {
        format!("taskers-{session_id}")
    }

    pub fn has_session(&self, session_id: &str) -> Result<bool> {
        let target = self.session_name(session_id);
        let output = self
            .command()
            .args(["has-session", "-t", &target])
            .output()
            .with_context(|| format!("failed to query tmux session {target}"))?;
        Ok(output.status.success())
    }

    pub fn kill_session(&self, session_id: &str) -> Result<()> {
        let target = self.session_name(session_id);
        let output = self
            .command()
            .args(["kill-session", "-t", &target])
            .output()
            .with_context(|| format!("failed to kill tmux session {target}"))?;
        if output.status.success() {
            return Ok(());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("can't find session") || stderr.contains("no server running") {
            return Ok(());
        }
        bail!("tmux kill-session failed: {}", stderr.trim());
    }

    pub fn attach_or_create(&self, session_id: &str, shell_args: &[String]) -> Result<()> {
        self.ensure_socket_parent()?;
        if !self.has_session(session_id)? {
            self.create_session(session_id, shell_args)?;
        }

        let target = self.session_name(session_id);
        #[cfg(unix)]
        {
            let error = self
                .command()
                .args(["attach-session", "-t", &target])
                .exec();
            Err(error).with_context(|| format!("failed to exec tmux attach for session {target}"))
        }
        #[cfg(not(unix))]
        {
            let status = self
                .command()
                .args(["attach-session", "-t", &target])
                .status()
                .with_context(|| format!("failed to spawn tmux attach for session {target}"))?;
            if status.success() {
                Ok(())
            } else {
                bail!("tmux attach failed with status {status}");
            }
        }
    }

    fn create_session(&self, session_id: &str, shell_args: &[String]) -> Result<()> {
        let target = self.session_name(session_id);
        let mut command = self.command();
        command.args(["new-session", "-d", "-s", &target]);
        if let Ok(cwd) = env::current_dir() {
            command.arg("-c").arg(cwd);
        }
        command.arg(self.new_session_command(shell_args)?);
        let output = command
            .output()
            .with_context(|| format!("failed to create tmux session {target}"))?;
        if output.status.success() {
            return Ok(());
        }
        bail!(
            "tmux new-session failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    fn new_session_command(&self, shell_args: &[String]) -> Result<String> {
        let wrapper = shell_wrapper_path()?;
        let mut parts = vec![
            "env".to_string(),
            "TASKERS_TMUX_CHILD=1".to_string(),
            "sh".to_string(),
        ];
        parts.push(shell_escape(wrapper.to_string_lossy().as_ref()));
        for arg in shell_args {
            parts.push(shell_escape(arg));
        }
        Ok(parts.join(" "))
    }

    fn ensure_socket_parent(&self) -> Result<()> {
        if let Some(parent) = self.socket_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create tmux socket directory {}",
                    parent.display()
                )
            })?;
        }
        Ok(())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.arg("-S").arg(&self.socket_path);
        command
    }
}

fn shell_wrapper_path() -> Result<PathBuf> {
    let root = env::var("TASKERS_SHELL_INTEGRATION_DIR")
        .context("TASKERS_SHELL_INTEGRATION_DIR is required for tmux shell entry")?;
    Ok(PathBuf::from(root).join("taskers-shell-wrapper.sh"))
}

fn shell_escape(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    let escaped = value.replace('\'', "'\"'\"'");
    format!("'{escaped}'")
}

#[cfg(test)]
mod tests {
    use super::{TmuxBackend, shell_escape};
    use std::path::PathBuf;

    #[test]
    fn tmux_detection_uses_requested_socket_path() {
        let backend = TmuxBackend::detect_with_socket(PathBuf::from("/tmp/taskers-test-tmux.sock"));
        if let Ok(backend) = backend {
            assert_eq!(
                backend.socket_path(),
                PathBuf::from("/tmp/taskers-test-tmux.sock").as_path()
            );
        }
    }

    #[test]
    fn shell_escape_handles_single_quotes() {
        assert_eq!(shell_escape(""), "''");
        assert_eq!(shell_escape("plain"), "'plain'");
        assert_eq!(shell_escape("it's"), "'it'\"'\"'s'");
    }
}
