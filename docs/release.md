# Release Prep

Use this checklist before publishing a new `taskers` release.

## 1. Finalize The Repo State

- Make sure the release work is recorded clearly in `jj`.
- Describe the current change with `jj desc -m "<type>: <summary>"` if needed.
- Split unrelated work into separate changes before publishing.
- Bump the workspace version in `Cargo.toml` and update any internal dependency version pins that still reference the previous release.

## 2. Refresh Release Assets

- Regenerate the README screenshots:

```bash
./scripts/capture_demo_screenshots.sh
```

- Review the updated files in `docs/screenshots/`.
- Update `README.md` if the screenshots or release notes no longer match the current UI.

## 3. Run Local Verification

- Run the full test suite:

```bash
cargo test
```

- Run the GTK smoke checks:

```bash
./scripts/smoke_taskers_ui.sh
./scripts/smoke_taskers_focus_churn.sh
```

- Build the Ghostty runtime asset that the published crate expects:

```bash
./scripts/build_ghostty_runtime_bundle.sh
```

The output asset name must match:

```text
taskers-ghostty-runtime-v<version>-<target>.tar.xz
```

- Dry-run crate publishing in dependency order:

```bash
cargo publish --dry-run -p taskers-domain
cargo publish --dry-run -p taskers-control
cargo publish --dry-run -p taskers-runtime
cargo publish --dry-run -p taskers-ghostty
cargo publish --dry-run -p taskers-cli
cargo publish --dry-run -p taskers
```

## 4. Publish

- Create a GitHub release draft tagged `v<version>`.
- Upload the matching Ghostty runtime bundle from `dist/`.
- Publish the crates to crates.io in the same order as the dry-run:

```bash
cargo publish -p taskers-domain
cargo publish -p taskers-control
cargo publish -p taskers-runtime
cargo publish -p taskers-ghostty
cargo publish -p taskers-cli
cargo publish -p taskers
```

## 5. Post-Publish Check

- Verify a clean install path:

```bash
cargo install taskers --locked
taskers --demo
```

- Confirm the published crate can bootstrap the matching runtime asset on first launch.
