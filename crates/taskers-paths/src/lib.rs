use std::{
    env,
    path::{Path, PathBuf},
};

pub const APP_ID: &str = "dev.taskers.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Linux,
    Macos,
    Other,
}

impl HostPlatform {
    pub const fn detect() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Default)]
struct EnvPaths {
    home: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
    xdg_data_home: Option<PathBuf>,
    xdg_runtime_dir: Option<PathBuf>,
    xdg_state_home: Option<PathBuf>,
    taskers_config_path: Option<PathBuf>,
    ghostty_gtk_runtime_dir: Option<PathBuf>,
    taskers_runtime_dir: Option<PathBuf>,
    taskers_session_path: Option<PathBuf>,
    taskers_socket_path: Option<PathBuf>,
    taskers_terminal_socket_path: Option<PathBuf>,
}

impl EnvPaths {
    fn current() -> Self {
        Self {
            home: env::var_os("HOME").map(PathBuf::from),
            xdg_cache_home: env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
            xdg_data_home: env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            xdg_runtime_dir: env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
            xdg_state_home: env::var_os("XDG_STATE_HOME").map(PathBuf::from),
            taskers_config_path: env::var_os("TASKERS_CONFIG_PATH").map(PathBuf::from),
            ghostty_gtk_runtime_dir: env::var_os("GHOSTTY_GTK_RUNTIME_DIR")
                .or_else(|| env::var_os("TASKERS_GHOSTTY_RUNTIME_DIR"))
                .map(PathBuf::from),
            taskers_runtime_dir: env::var_os("TASKERS_RUNTIME_DIR").map(PathBuf::from),
            taskers_session_path: env::var_os("TASKERS_SESSION_PATH").map(PathBuf::from),
            taskers_socket_path: env::var_os("TASKERS_SOCKET_PATH").map(PathBuf::from),
            taskers_terminal_socket_path: env::var_os("TASKERS_TERMINAL_SOCKET_PATH")
                .map(PathBuf::from),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskersPaths {
    config_dir: PathBuf,
    state_dir: PathBuf,
    cache_dir: PathBuf,
    data_dir: PathBuf,
    shell_runtime_dir: PathBuf,
    ghostty_runtime_dir: PathBuf,
    socket_path: PathBuf,
    terminal_socket_path: PathBuf,
    session_path: PathBuf,
    config_path: PathBuf,
    theme_dir: PathBuf,
}

impl TaskersPaths {
    pub fn detect() -> Self {
        Self::from_env(HostPlatform::detect(), &EnvPaths::current())
    }

    fn from_env(platform: HostPlatform, env_paths: &EnvPaths) -> Self {
        let (config_dir, config_path, state_dir) =
            if let Some(config_path) = env_paths.taskers_config_path.clone() {
                let config_dir = config_path
                    .parent()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| temp_root().join("config"));
                let state_dir = env_paths
                    .taskers_session_path
                    .clone()
                    .and_then(|path| path.parent().map(PathBuf::from))
                    .unwrap_or_else(|| platform_state_dir(platform, env_paths));
                (config_dir, config_path, state_dir)
            } else {
                let config_dir = platform_config_dir(platform, env_paths);
                let state_dir = platform_state_dir(platform, env_paths);
                let config_path = config_dir.join("config.json");
                (config_dir, config_path, state_dir)
            };

        let cache_dir = platform_cache_dir(platform, env_paths);
        let data_dir = platform_data_dir(platform, env_paths);
        let shell_runtime_dir = shell_runtime_dir(platform, env_paths, &cache_dir);
        let ghostty_runtime_dir = env_paths
            .ghostty_gtk_runtime_dir
            .clone()
            .unwrap_or_else(|| data_dir.join("ghostty"));
        let socket_path = env_paths
            .taskers_socket_path
            .clone()
            .unwrap_or_else(|| socket_path(platform, &cache_dir));
        let terminal_socket_path = env_paths
            .taskers_terminal_socket_path
            .clone()
            .unwrap_or_else(|| terminal_socket_path(platform, env_paths, &cache_dir));
        let session_path = env_paths
            .taskers_session_path
            .clone()
            .unwrap_or_else(|| state_dir.join("session.json"));

        Self {
            theme_dir: config_dir.join("themes"),
            config_dir,
            state_dir,
            cache_dir,
            data_dir,
            shell_runtime_dir,
            ghostty_runtime_dir,
            socket_path,
            terminal_socket_path,
            session_path,
            config_path,
        }
    }

    pub fn config_dir(&self) -> &PathBuf {
        &self.config_dir
    }

    pub fn state_dir(&self) -> &PathBuf {
        &self.state_dir
    }

    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    pub fn shell_runtime_dir(&self) -> &PathBuf {
        &self.shell_runtime_dir
    }

    pub fn ghostty_runtime_dir(&self) -> &PathBuf {
        &self.ghostty_runtime_dir
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }

    pub fn terminal_socket_path(&self) -> &PathBuf {
        &self.terminal_socket_path
    }

    pub fn session_path(&self) -> &PathBuf {
        &self.session_path
    }

    pub fn config_path(&self) -> &PathBuf {
        &self.config_path
    }

    pub fn theme_dir(&self) -> &PathBuf {
        &self.theme_dir
    }
}

pub fn default_socket_path() -> PathBuf {
    TaskersPaths::detect().socket_path
}

pub fn default_session_path() -> PathBuf {
    TaskersPaths::detect().session_path
}

pub fn default_terminal_socket_path() -> PathBuf {
    TaskersPaths::detect().terminal_socket_path
}

pub fn default_config_path() -> PathBuf {
    TaskersPaths::detect().config_path
}

pub fn default_theme_dir() -> PathBuf {
    TaskersPaths::detect().theme_dir
}

pub fn default_shell_runtime_dir() -> PathBuf {
    TaskersPaths::detect().shell_runtime_dir
}

pub fn default_ghostty_runtime_dir() -> PathBuf {
    TaskersPaths::detect().ghostty_runtime_dir
}

pub fn default_release_install_root() -> PathBuf {
    TaskersPaths::detect().data_dir.join("releases")
}

fn platform_config_dir(platform: HostPlatform, env_paths: &EnvPaths) -> PathBuf {
    match platform {
        HostPlatform::Macos => home_library_dir(env_paths, "Application Support"),
        HostPlatform::Linux => env_paths
            .xdg_config_home
            .clone()
            .map(|path| path.join("taskers"))
            .or_else(|| {
                env_paths
                    .home
                    .clone()
                    .map(|path| path.join(".config").join("taskers"))
            })
            .unwrap_or_else(|| temp_root().join("config")),
        HostPlatform::Other => env_paths
            .home
            .clone()
            .map(|path| path.join(".taskers"))
            .unwrap_or_else(|| temp_root().join("config")),
    }
}

fn platform_state_dir(platform: HostPlatform, env_paths: &EnvPaths) -> PathBuf {
    match platform {
        HostPlatform::Macos => home_library_dir(env_paths, "Application Support"),
        HostPlatform::Linux => env_paths
            .xdg_state_home
            .clone()
            .map(|path| path.join("taskers"))
            .or_else(|| {
                env_paths
                    .home
                    .clone()
                    .map(|path| path.join(".local").join("state").join("taskers"))
            })
            .unwrap_or_else(|| temp_root().join("state")),
        HostPlatform::Other => env_paths
            .home
            .clone()
            .map(|path| path.join(".taskers"))
            .unwrap_or_else(|| temp_root().join("state")),
    }
}

fn platform_cache_dir(platform: HostPlatform, env_paths: &EnvPaths) -> PathBuf {
    match platform {
        HostPlatform::Macos => home_library_cache_dir(env_paths),
        HostPlatform::Linux => env_paths
            .xdg_cache_home
            .clone()
            .map(|path| path.join("taskers"))
            .or_else(|| {
                env_paths
                    .home
                    .clone()
                    .map(|path| path.join(".cache").join("taskers"))
            })
            .unwrap_or_else(|| temp_root().join("cache")),
        HostPlatform::Other => env_paths
            .home
            .clone()
            .map(|path| path.join(".taskers").join("cache"))
            .unwrap_or_else(|| temp_root().join("cache")),
    }
}

fn platform_data_dir(platform: HostPlatform, env_paths: &EnvPaths) -> PathBuf {
    match platform {
        HostPlatform::Macos => home_library_dir(env_paths, "Application Support"),
        HostPlatform::Linux => env_paths
            .xdg_data_home
            .clone()
            .map(|path| path.join("taskers"))
            .or_else(|| {
                env_paths
                    .home
                    .clone()
                    .map(|path| path.join(".local").join("share").join("taskers"))
            })
            .unwrap_or_else(|| temp_root().join("data")),
        HostPlatform::Other => env_paths
            .home
            .clone()
            .map(|path| path.join(".taskers").join("data"))
            .unwrap_or_else(|| temp_root().join("data")),
    }
}

fn shell_runtime_dir(platform: HostPlatform, env_paths: &EnvPaths, cache_dir: &Path) -> PathBuf {
    if let Some(path) = env_paths.taskers_runtime_dir.clone() {
        return path.join("shell");
    }

    match platform {
        HostPlatform::Linux => env_paths
            .xdg_runtime_dir
            .clone()
            .map(|path| path.join("taskers").join("shell"))
            .unwrap_or_else(|| env::temp_dir().join("taskers-runtime").join("shell")),
        HostPlatform::Macos => cache_dir.join("runtime").join("shell"),
        HostPlatform::Other => env::temp_dir().join("taskers-runtime").join("shell"),
    }
}

fn socket_path(platform: HostPlatform, cache_dir: &Path) -> PathBuf {
    match platform {
        HostPlatform::Macos => cache_dir.join("control.sock"),
        HostPlatform::Linux | HostPlatform::Other => PathBuf::from("/tmp/taskers.sock"),
    }
}

fn terminal_socket_path(platform: HostPlatform, env_paths: &EnvPaths, cache_dir: &Path) -> PathBuf {
    match platform {
        HostPlatform::Linux => env_paths
            .xdg_runtime_dir
            .clone()
            .map(|path| path.join("taskers").join("terminal.sock"))
            .unwrap_or_else(|| PathBuf::from("/tmp/taskers-terminal.sock")),
        HostPlatform::Macos => cache_dir.join("terminal.sock"),
        HostPlatform::Other => PathBuf::from("/tmp/taskers-terminal.sock"),
    }
}

fn home_library_dir(env_paths: &EnvPaths, leaf: &str) -> PathBuf {
    env_paths
        .home
        .clone()
        .map(|path| path.join("Library").join(leaf).join(APP_ID))
        .unwrap_or_else(|| temp_root().join(leaf.replace(' ', "-").to_ascii_lowercase()))
}

fn home_library_cache_dir(env_paths: &EnvPaths) -> PathBuf {
    env_paths
        .home
        .clone()
        .map(|path| path.join("Library").join("Caches").join(APP_ID))
        .unwrap_or_else(|| temp_root().join("cache"))
}

fn temp_root() -> PathBuf {
    env::temp_dir().join("taskers")
}

#[cfg(test)]
mod tests {
    use super::{APP_ID, EnvPaths, HostPlatform, TaskersPaths};
    use std::path::PathBuf;

    #[test]
    fn macos_paths_use_library_directories() {
        let env = EnvPaths {
            home: Some(PathBuf::from("/Users/notes")),
            ..EnvPaths::default()
        };
        let paths = TaskersPaths::from_env(HostPlatform::Macos, &env);

        assert_eq!(
            paths.config_path(),
            &PathBuf::from(format!(
                "/Users/notes/Library/Application Support/{APP_ID}/config.json"
            ))
        );
        assert_eq!(
            paths.session_path(),
            &PathBuf::from(format!(
                "/Users/notes/Library/Application Support/{APP_ID}/session.json"
            ))
        );
        assert_eq!(
            paths.socket_path(),
            &PathBuf::from(format!("/Users/notes/Library/Caches/{APP_ID}/control.sock"))
        );
        assert_eq!(
            paths.terminal_socket_path(),
            &PathBuf::from(format!(
                "/Users/notes/Library/Caches/{APP_ID}/terminal.sock"
            ))
        );
        assert_eq!(
            paths.shell_runtime_dir(),
            &PathBuf::from(format!(
                "/Users/notes/Library/Caches/{APP_ID}/runtime/shell"
            ))
        );
    }

    #[test]
    fn linux_paths_preserve_xdg_defaults() {
        let env = EnvPaths {
            home: Some(PathBuf::from("/home/notes")),
            xdg_config_home: Some(PathBuf::from("/tmp/config")),
            xdg_state_home: Some(PathBuf::from("/tmp/state")),
            xdg_cache_home: Some(PathBuf::from("/tmp/cache")),
            xdg_data_home: Some(PathBuf::from("/tmp/data")),
            xdg_runtime_dir: Some(PathBuf::from("/tmp/runtime")),
            ..EnvPaths::default()
        };
        let paths = TaskersPaths::from_env(HostPlatform::Linux, &env);

        assert_eq!(
            paths.config_path(),
            &PathBuf::from("/tmp/config/taskers/config.json")
        );
        assert_eq!(
            paths.session_path(),
            &PathBuf::from("/tmp/state/taskers/session.json")
        );
        assert_eq!(
            paths.ghostty_runtime_dir(),
            &PathBuf::from("/tmp/data/taskers/ghostty")
        );
        assert_eq!(
            paths.shell_runtime_dir(),
            &PathBuf::from("/tmp/runtime/taskers/shell")
        );
        assert_eq!(paths.socket_path(), &PathBuf::from("/tmp/taskers.sock"));
        assert_eq!(
            paths.terminal_socket_path(),
            &PathBuf::from("/tmp/runtime/taskers/terminal.sock")
        );
    }

    #[test]
    fn explicit_overrides_win() {
        let env = EnvPaths {
            taskers_config_path: Some(PathBuf::from("/work/config.json")),
            taskers_session_path: Some(PathBuf::from("/work/session.json")),
            taskers_socket_path: Some(PathBuf::from("/work/control.sock")),
            taskers_terminal_socket_path: Some(PathBuf::from("/work/terminal.sock")),
            taskers_runtime_dir: Some(PathBuf::from("/work/runtime")),
            ghostty_gtk_runtime_dir: Some(PathBuf::from("/work/ghostty")),
            ..EnvPaths::default()
        };
        let paths = TaskersPaths::from_env(HostPlatform::Macos, &env);

        assert_eq!(paths.config_path(), &PathBuf::from("/work/config.json"));
        assert_eq!(paths.session_path(), &PathBuf::from("/work/session.json"));
        assert_eq!(paths.socket_path(), &PathBuf::from("/work/control.sock"));
        assert_eq!(
            paths.terminal_socket_path(),
            &PathBuf::from("/work/terminal.sock")
        );
        assert_eq!(
            paths.shell_runtime_dir(),
            &PathBuf::from("/work/runtime/shell")
        );
        assert_eq!(paths.ghostty_runtime_dir(), &PathBuf::from("/work/ghostty"));
    }

    #[test]
    fn generic_gtk_runtime_dir_alias_overrides_default_ghostty_runtime_dir() {
        let env = EnvPaths {
            xdg_data_home: Some(PathBuf::from("/tmp/data")),
            ghostty_gtk_runtime_dir: Some(PathBuf::from("/work/generic-ghostty")),
            ..EnvPaths::default()
        };
        let paths = TaskersPaths::from_env(HostPlatform::Linux, &env);

        assert_eq!(
            paths.ghostty_runtime_dir(),
            &PathBuf::from("/work/generic-ghostty")
        );
    }

    #[test]
    fn config_path_override_only_changes_config_derived_paths() {
        let env = EnvPaths {
            home: Some(PathBuf::from("/home/notes")),
            xdg_config_home: Some(PathBuf::from("/tmp/config")),
            xdg_state_home: Some(PathBuf::from("/tmp/state")),
            xdg_cache_home: Some(PathBuf::from("/tmp/cache")),
            xdg_data_home: Some(PathBuf::from("/tmp/data")),
            xdg_runtime_dir: Some(PathBuf::from("/tmp/runtime")),
            taskers_config_path: Some(PathBuf::from("/work/taskers/config.json")),
            ..EnvPaths::default()
        };
        let paths = TaskersPaths::from_env(HostPlatform::Linux, &env);

        assert_eq!(paths.config_dir(), &PathBuf::from("/work/taskers"));
        assert_eq!(
            paths.config_path(),
            &PathBuf::from("/work/taskers/config.json")
        );
        assert_eq!(paths.theme_dir(), &PathBuf::from("/work/taskers/themes"));

        assert_eq!(paths.state_dir(), &PathBuf::from("/tmp/state/taskers"));
        assert_eq!(
            paths.session_path(),
            &PathBuf::from("/tmp/state/taskers/session.json")
        );
        assert_eq!(paths.cache_dir(), &PathBuf::from("/tmp/cache/taskers"));
        assert_eq!(paths.data_dir(), &PathBuf::from("/tmp/data/taskers"));
        assert_eq!(
            paths.shell_runtime_dir(),
            &PathBuf::from("/tmp/runtime/taskers/shell")
        );
        assert_eq!(
            paths.ghostty_runtime_dir(),
            &PathBuf::from("/tmp/data/taskers/ghostty")
        );
        assert_eq!(paths.socket_path(), &PathBuf::from("/tmp/taskers.sock"));
        assert_eq!(
            paths.terminal_socket_path(),
            &PathBuf::from("/tmp/runtime/taskers/terminal.sock")
        );
    }

    #[test]
    fn release_install_roots_follow_platform_defaults() {
        let mac = EnvPaths {
            home: Some(PathBuf::from("/Users/notes")),
            ..EnvPaths::default()
        };
        let linux = EnvPaths {
            home: Some(PathBuf::from("/home/notes")),
            xdg_data_home: Some(PathBuf::from("/tmp/data")),
            ..EnvPaths::default()
        };

        assert_eq!(
            TaskersPaths::from_env(HostPlatform::Macos, &mac)
                .data_dir()
                .join("releases"),
            PathBuf::from(format!(
                "/Users/notes/Library/Application Support/{APP_ID}/releases"
            ))
        );
        assert_eq!(
            TaskersPaths::from_env(HostPlatform::Linux, &linux)
                .data_dir()
                .join("releases"),
            PathBuf::from("/tmp/data/taskers/releases")
        );
    }
}
