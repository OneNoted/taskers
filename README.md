# Taskers

Taskers is a Linux-first terminal workspace for agent-heavy work. It combines:

- top-level workspace windows that behave like a tiling canvas
- panes inside each window
- tabs inside each pane
- an attention rail for unread, waiting, error, and completed work
- a local control CLI for notifications, agent state, browser automation, and debugging

The active product lives at the repo root. Archived pre-cutover GTK/AppKit code
is kept under `taskers-old/` for reference only.

## Documentation

- [Daily usage](docs/usage.md)
- [Notifications and attention](docs/notifications.md)
- [Taskersctl operator guide](docs/taskersctl.md)
- [Release checklist](docs/release.md)

## Install

Linux (`x86_64-unknown-linux-gnu`):

```bash
cargo install taskers --locked
taskers
```

The first launch downloads the exact version-matched Linux bundle from the
tagged GitHub release. The Linux app requires GTK4/libadwaita plus the host
WebKitGTK 6.0 runtime.

Mainline macOS support is currently not shipped from this repo root.

## First 5 Minutes

Start Taskers:

```bash
taskers
```

Keep the workspace model straight:

- A workspace contains top-level workspace windows.
- A workspace window contains panes.
- A pane contains tabs.
- Scrolling/panning is a workspace-window concern. Splits and tabs stay local to the current window.

Open a terminal in Taskers and emit a test notification:

```bash
taskersctl notify --title "Taskers" --body "Hello from the current pane"
```

You should see:

- an unread badge in the sidebar
- a notification row in the attention rail
- a desktop banner unless the target is already visible and notification suppression is enabled

Try the built-in shell helpers from a Taskers terminal:

```bash
taskers_waiting "Need review"
taskers_done "Finished"
taskers_error "Build failed"
```

Open a browser pane from the pane controls, then inspect it from the same Taskers terminal:

```bash
taskersctl browser snapshot
taskersctl browser get title
```

## Develop

On Ubuntu 24.04, install the Linux UI dependencies first:

```bash
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev xvfb
```

Run the app directly:

```bash
cargo run -p taskers-gtk --bin taskers-gtk
```

Point the desktop launcher at the repo-local dev build:

```bash
bash scripts/install-dev-desktop-entry.sh
```

Run the headless baseline smoke:

```bash
TASKERS_TERMINAL_BACKEND=mock \
bash scripts/headless-smoke.sh \
  ./target/debug/taskers-gtk \
  --smoke-script baseline \
  --diagnostic-log stderr \
  --quit-after-ms 5000
```

For publishing and release prep, use [docs/release.md](docs/release.md).
