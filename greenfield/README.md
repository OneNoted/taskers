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

Run the scripted baseline smoke with isolated XDG and runtime paths:

```bash
TMPDIR="$(mktemp -d)"
XDG_CONFIG_HOME="$TMPDIR/config" \
XDG_DATA_HOME="$TMPDIR/data" \
XDG_STATE_HOME="$TMPDIR/state" \
XDG_CACHE_HOME="$TMPDIR/cache" \
TASKERS_RUNTIME_DIR="$TMPDIR/runtime" \
TASKERS_GHOSTTY_RUNTIME_DIR="$TMPDIR/ghostty" \
cargo run -p taskers -- --smoke-script baseline --diagnostic-log stderr --quit-after-ms 5000
```

Diagnostics can also be written to a file:

```bash
cargo run -p taskers -- --smoke-script baseline --diagnostic-log /tmp/taskers-greenfield.log
```

Baseline comparison checklist:

- Startup logs runtime capability states instead of silently falling back.
- Initial window attach and snapshot sync are recorded.
- Browser pane creation is logged through the GTK portal runtime.
- Browser title metadata is observed from the native surface.
- Terminal split records the current explicit fallback state instead of pretending native hosting works.
- Final smoke output records pane counts, active pane, and exit timing.

Manual interactive checklist:

- Launch `cargo run -p taskers`.
- Split one browser pane and one terminal pane.
- Resize the window and confirm the browser surface stays aligned with the shell chrome.
- Move focus between panes and confirm the active-pane highlight follows.
- Confirm the terminal pane still reports the GTK3/GTK4 Ghostty fallback honestly.
