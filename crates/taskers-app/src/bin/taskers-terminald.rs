use std::{env, path::PathBuf};

use anyhow::{Context, Result};
use taskers_runtime::TerminalSessionDaemon;

fn main() -> Result<()> {
    let socket = parse_socket_arg()
        .or_else(|| env::var_os("TASKERS_TERMINAL_SOCKET").map(PathBuf::from))
        .context("missing --socket for taskers-terminald")?;
    TerminalSessionDaemon::new().serve(&socket)
}

fn parse_socket_arg() -> Option<PathBuf> {
    let mut args = env::args_os().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--socket" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}
