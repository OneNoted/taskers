# taskers

Taskers is a Linux-first terminal workspace for agent-heavy work. It gives you Niri-style top-level windows, local pane splits, and an attention sidebar so active, waiting, and completed terminal work stays visible.

![Taskers workspace list and attention sidebar](docs/screenshots/demo-attention.png)

![Taskers split workspace window](docs/screenshots/demo-layout.png)

## Try it

```bash
cargo install taskers --locked
taskers --demo
```

The first launch bootstraps the matching Ghostty runtime bundle when needed.

## Develop

```bash
cargo run -p taskers -- --demo
./scripts/smoke_taskers_ui.sh
```

Release checklist: [docs/release.md](docs/release.md)
