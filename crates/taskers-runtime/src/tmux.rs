use std::{
    path::{Path, PathBuf},
    process::Command,
};

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
}

#[cfg(test)]
mod tests {
    use super::TmuxBackend;
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
}
