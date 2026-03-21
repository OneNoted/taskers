# taskers

Taskers is a Linux-first terminal workspace for agent-heavy work. It provides
Niri-style top-level windows, local pane splits, tabs inside panes, and an
attention rail for active and completed work.

The active product lives at the repo root. Archived pre-cutover GTK/AppKit code
is kept under `taskers-old/` for reference only.

## Try it

Linux (`x86_64-unknown-linux-gnu`):

```bash
cargo install taskers --locked
taskers
```

The first launch downloads the exact version-matched Linux bundle from the tagged
GitHub release. The Linux app requires GTK4/libadwaita plus the host WebKitGTK
6.0 runtime.

Mainline macOS support is currently not shipped from this repo root.

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

Release checklist: [docs/release.md](docs/release.md)
