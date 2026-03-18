# taskers

Taskers is a cross-platform terminal workspace for agent-heavy work. It gives you Niri-style top-level windows, local pane splits, and an attention sidebar so active, waiting, and completed terminal work stays visible.

![Taskers workspace list and attention sidebar](docs/screenshots/demo-attention.png)

![Taskers split workspace window](docs/screenshots/demo-layout.png)

## Try it

Linux (`x86_64-unknown-linux-gnu`):

```bash
cargo install taskers --locked
taskers --demo
```

The first launch downloads the exact version-matched Linux bundle from the tagged GitHub release.
The Linux app now requires the host WebKitGTK 6.0 runtime in addition to GTK4/libadwaita.

macOS:

- Download the signed `Taskers-v<version>-universal2.dmg` from [GitHub Releases](https://github.com/OneNoted/taskers/releases).
- Drag `Taskers.app` into `Applications`, then launch it normally from Finder or Spotlight.

## Develop

On Ubuntu 24.04, install the Linux UI dependencies first:

```bash
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev
```

```bash
cargo run -p taskers-gtk --bin taskers-gtk -- --demo
```

Release checklist: [docs/release.md](docs/release.md)
