# Taskers Greenfield Rewrite

This nested workspace is the bootstrap implementation for the Dioxus-based rewrite.

It is intentionally isolated from the legacy GTK/AppKit workspace at the repo root so the
new architecture can evolve without disturbing the existing product.

Current scope:

- shared Rust core for workspace/pane/surface state
- Dioxus desktop shell with CSS-driven chrome
- native browser child-webview portal mounted against the Dioxus window
- explicit terminal host seam for future Ghostty integration

Run it from this workspace:

```bash
cargo run -p taskers
```

