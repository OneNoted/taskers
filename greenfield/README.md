# Taskers Greenfield Rewrite

This nested workspace is the bootstrap implementation for the shared Dioxus shell rewrite.

It is intentionally isolated from the legacy GTK/AppKit workspace at the repo root so the
new architecture can evolve without disturbing the existing product.

Current scope:

- shared Rust core for workspace/pane/surface state
- shared Dioxus shell rendered through LiveView inside a GTK4/libadwaita host
- Linux GTK4 portal runtime that mounts browser and Ghostty pane bodies into the shared shell
- legacy Taskers-inspired shell styling instead of the old greenfield prototype chrome
- startup/runtime bootstrap that scrubs inherited terminal env, installs shell integration, and probes Ghostty runtime availability

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
cargo run -p taskers -- --smoke-script baseline --diagnostic-log stderr --quit-after-ms 5000
```

Diagnostics can also be written to a file:

```bash
cargo run -p taskers -- --smoke-script baseline --diagnostic-log /tmp/taskers-greenfield.log
```

For a CI-like launch check that avoids the current interactive Ghostty abort on this machine, use the headless helper:

```bash
greenfield/scripts/headless-smoke.sh \
  ./greenfield/target/debug/taskers \
  --smoke-script baseline \
  --diagnostic-log stderr \
  --quit-after-ms 5000
```

Baseline comparison checklist:

- Startup logs runtime capability states instead of silently falling back.
- Initial GTK4 host attach and snapshot sync are recorded.
- Browser pane creation is logged through the GTK4 portal runtime.
- Browser title metadata is observed from the native surface.
- Terminal pane creation is attempted through the Ghostty host path.
- Final smoke output records pane counts, active pane, and exit timing.

Manual interactive checklist:

- Launch `cargo run -p taskers`.
- Split one browser pane and one terminal pane.
- Resize the window and confirm the native surfaces stay aligned with the shell chrome.
- Move focus between panes and confirm the active-pane highlight follows.

Current blocker:

- On this machine, direct interactive launch can still abort inside the Ghostty bridge during startup before the shared shell becomes usable. This matches the current comparator result and appears to be a Ghostty runtime issue rather than a GTK3/GTK4 host mismatch.
- Headless smoke with `dbus-run-session + xvfb-run + LIBGL_ALWAYS_SOFTWARE=1` is the current reliable validation path until that Ghostty startup abort is fixed.
