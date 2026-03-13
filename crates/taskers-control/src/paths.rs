use std::path::PathBuf;

pub fn default_socket_path() -> PathBuf {
    std::env::var_os("TASKERS_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/taskers.sock"))
}
