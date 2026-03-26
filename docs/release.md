# Release Prep

Use this checklist before publishing a new `taskers` Linux release.

## 1. Finalize The Repo State

- Make sure the release work is recorded clearly in `jj`.
- Describe the current change with `jj desc -m "<type>: <summary>"` if needed.
- Split unrelated work into separate changes before publishing.
- Bump the workspace version in `Cargo.toml` and update any internal dependency version pins that still reference the previous release.

## 2. Run Local Verification

- On Ubuntu 24.04, install the Linux UI dependencies first:

```bash
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev xvfb
```

- Run the full active workspace test suite:

```bash
cargo test
```

- Build the mainline app and run the headless smoke:

```bash
cargo build -p taskers-gtk --bin taskers-gtk
TASKERS_TERMINAL_BACKEND=mock \
  bash scripts/headless-smoke.sh \
  ./target/debug/taskers-gtk \
  --smoke-script baseline \
  --diagnostic-log stderr \
  --quit-after-ms 5000
```

- Build the Linux bundle and verify the published launcher path:

```bash
bash scripts/build_linux_bundle.sh
bash scripts/smoke_linux_release_launcher.sh
```

The output asset name must match:

```text
taskers-linux-bundle-v<version>-<target>.tar.xz
```

- Build the release manifest from the generated assets:

```bash
python3 scripts/build_release_manifest.py
```

- Dry-run the leaf crates that do not depend on unpublished workspace siblings:

```bash
cargo publish --dry-run -p taskers-domain
cargo publish --dry-run -p taskers-paths
```

- After you bump the workspace to a new unpublished version, `cargo publish --dry-run` for dependent crates will still resolve dependencies from crates.io and fail until the earlier crates are actually published. That failure is expected for:
  - `taskers-control`
  - `taskers-runtime`
  - `taskers-ghostty`
  - `taskers-cli`
  - `taskers`

## 3. Publish

- Push the release tag so GitHub Actions can assemble the assets and attach them to a draft GitHub release.
- Confirm the draft release tagged `v<version>` contains:
  - `taskers-manifest-v<version>.json`
  - `taskers-linux-bundle-v<version>-x86_64-unknown-linux-gnu.tar.xz`
- Publish the GitHub release so the launcher assets are publicly downloadable before publishing the crates.
- Publish the crates to crates.io in dependency order:

```bash
cargo publish -p taskers-domain
cargo publish -p taskers-paths
cargo publish -p taskers-control
cargo publish -p taskers-runtime
cargo publish -p taskers-ghostty
cargo publish -p taskers-cli
cargo publish -p taskers
```

## 4. Post-Publish Check

- Verify the Linux launcher install:

```bash
cargo install taskers --locked
taskers
```

- Confirm the published Linux launcher downloads the exact version-matched bundle on first launch.
- Confirm `cargo install taskers-cli --bin taskersctl --locked` still works as the standalone helper path.
- Confirm `cargo install taskers --locked` on macOS fails with the Linux-only guidance from the launcher crate.
