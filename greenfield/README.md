# Taskers Greenfield Rewrite

This nested workspace is the bootstrap implementation for the Dioxus-based rewrite.

It is intentionally isolated from the legacy GTK/AppKit workspace at the repo root so the
new architecture can evolve without disturbing the existing product.

Current scope:

- shared Rust core for workspace/pane/surface state
- Dioxus desktop shell with CSS-driven chrome
- Linux GTK portal runtime that mounts browser panes into the Dioxus window
- startup/runtime bootstrap that scrubs inherited terminal env and installs shell integration
- explicit terminal host fallback until the Ghostty GTK4 bridge is reconciled with the GTK3 Dioxus host

Run it from this workspace:

```bash
cargo run -p taskers
```
