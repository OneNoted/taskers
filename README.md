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
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev
cargo install taskers --locked
taskers
```

`cargo install taskers` installs the Linux launcher entrypoint. On first launch,
that launcher ensures the version-matched Linux app bundle is present under the
user data directory and then starts the bundled `taskers-gtk` host plus its
helper binaries. The first launch also bootstraps the version-matched Ghostty
runtime assets when needed.

The Linux app requires GTK4/libadwaita plus the host WebKitGTK 6.0 development
packages at install time and the WebKitGTK 6.0 runtime at launch time. Taskers
handles terminal-session persistence itself on Linux through its internal
terminal sidecar; if the sidecar is unavailable, Taskers falls back to fresh
shells and warns that persistence is unavailable.

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

Enable shell completions for `taskersctl`:

```bash
mkdir -p ~/.local/share/bash-completion/completions
taskersctl completion bash > ~/.local/share/bash-completion/completions/taskersctl

mkdir -p ~/.zsh/completions
taskersctl completion zsh > ~/.zsh/completions/_taskersctl

mkdir -p ~/.config/fish/completions
taskersctl completion fish > ~/.config/fish/completions/taskersctl.fish
```

Those generated scripts also complete live `--workspace`, `--pane`, and `--surface` ids when Taskers is running.

## Develop

On Ubuntu 24.04, install the Linux UI dependencies first:

```bash
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev xvfb
```

Install the app into Cargo's bin directory:

```bash
cargo install --path crates/taskers-app --force
```

That installs the repo-local binaries directly into Cargo's bin directory:

- `taskers`
- `taskers-gtk`
- `taskersctl`
- `taskers-terminald`

For repo-local verification, prefer the directly installed GTK host:

```bash
taskers-gtk
```

If plain `taskers` still resolves to `~/.local/bin/taskers`, that is the
launcher-managed bundle path and can lag behind your repo build. Use
`taskers-gtk` or the absolute Cargo bin path for feature testing unless you
intentionally refreshed the launcher-managed release bundle too.

`taskers-terminald` is an internal helper binary used for persistent terminal
sessions. You normally do not launch it by hand.

Run the headless baseline smoke:

```bash
TASKERS_TERMINAL_BACKEND=mock \
bash scripts/headless-smoke.sh \
  "$(command -v taskers-gtk)" \
  --smoke-script baseline \
  --diagnostic-log stderr \
  --quit-after-ms 5000
```

For publishing and release prep, use [docs/release.md](docs/release.md).
