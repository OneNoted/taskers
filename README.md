# taskers

`taskers` is an agent-first terminal workspace app scaffolded around a Linux-first Rust shell, a flexible terminal backend boundary, and CMUX-style workspace/pane attention routing.

## Workspace layout

- `taskers-domain`: UI-agnostic workspace, pane, layout, signal, and persistence model
- `taskers-control`: local control protocol, JSON framing, in-memory controller, and Unix socket server/client
- `taskers-runtime`: PTY/session foundation and explicit OSC signal parser
- `taskers-ghostty`: terminal backend abstraction and libghostty probe/fallback surface
- `taskers-cli`: CLI for querying and mutating the app over the local control socket
- `taskers-app`: GTK4/libadwaita shell that owns controller state, session persistence, and the local control socket

## Quick start

```bash
cargo run -p taskers-app
```

```bash
cargo run -p taskers-app -- --demo
```

```bash
cargo run -p taskers-cli -- query status --socket /tmp/taskers.sock
```

```bash
cargo run -p taskers-cli -- pane split --workspace <workspace-id> --axis vertical
```

## Repository setup

If you use `jj` in this repository, run the repo setup script once per clone:

```bash
./scripts/setup-jj.sh
```

The vendored Ghostty tree includes approved upstream assets larger than Jujutsu's default
1 MiB snapshot limit. The setup script raises the repo-local limit just enough to snapshot
the current vendored tree without changing your global `jj` behavior.

When refreshing `vendor/ghostty`, run the large-file check before pushing:

```bash
python3 scripts/check_ghostty_vendor_large_files.py
```

This fails if new files over 1 MiB appear outside the approved Ghostty allowlist, or if an
approved file grows past the repo-local `jj` limit.

## Install locally

```bash
cargo run -p taskers-cli -- install
```

This installs the `taskers` binary into Cargo's bin directory and writes a desktop entry plus icon into your local XDG application directories so it shows up in Linux app launchers.

## Current status

This foundation now includes:

- domain model, layout tree, attention state reducer, and persistence snapshot
- explicit control protocol and Unix socket transport
- PTY spawning foundation and explicit OSC marker parsing
- GTK shell with live workspace switching, pane focus, split actions, autosave, and an app-hosted control server
- real shell sessions per terminal pane, with live output streaming and input-on-enter in the pane UI
- session load/save support with configurable session and socket paths

The actual libghostty embedding work is intentionally isolated behind `taskers-ghostty` so the app shell and domain logic stay stable if the integration strategy changes.
