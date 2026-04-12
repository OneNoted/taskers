# Release Prep

This guide is for release maintainers. For everyday usage and operator flows,
start with the root [README](../README.md), [daily usage](usage.md), and
[notifications guide](notifications.md).

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
cargo build -p taskers --bin taskers --bin taskers-gtk
TASKERS_TERMINAL_BACKEND=mock \
  bash scripts/headless-smoke.sh \
  ./target/debug/taskers \
  --smoke-script baseline \
  --diagnostic-log stderr \
  --quit-after-ms 5000
```

- Verify the installed Linux package layout:

```bash
bash scripts/smoke_linux_release_launcher.sh
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

Before publishing, also verify the operator path still works:

```bash
cargo run -p taskers-cli -- --help
cargo run -p taskers-cli -- notify --help
```

## 3. Publish

- Push the release tag so GitHub Actions can assemble the assets and attach them to a draft GitHub release.
- Confirm the draft release tagged `v<version>` contains:
  - `taskers-ghostty-runtime-v<version>-x86_64-unknown-linux-gnu.tar.xz`
  - `taskers-linux-bundle-v<version>-x86_64-unknown-linux-gnu.tar.xz`
  - `taskers-linux-bundle-x86_64-unknown-linux-gnu.tar.xz`
  - `taskers-manifest-v<version>.json`
  - `taskers-manifest.json`
- Publish the GitHub release so the Ghostty runtime asset is publicly downloadable before publishing the crates.
- Publish the crates to crates.io in dependency order:

```bash
cargo publish -p taskers-domain
cargo publish -p taskers-paths
cargo publish -p taskers-control
cargo publish -p taskers-runtime
cargo publish -p taskers-ghostty
cargo publish -p taskers-core
cargo publish -p taskers-shell-core
cargo publish -p taskers-cli
cargo publish -p taskers-host
cargo publish -p taskers-shell
cargo publish -p taskers
```

- Bump `packaging/aur/taskers-bin/PKGBUILD` `pkgver` to the new stable release
  version, update the release bundle SHA-256 in `sha256sums`, and regenerate
  `packaging/aur/*/.SRCINFO`.
- Push the updated `taskers-bin` and `taskers-git` package directories to their
  matching AUR repos. `taskers-bin` should continue pointing at the explicit
  versioned release asset that matches its committed `pkgver`, while the stable
  `releases/latest/download/taskers-linux-bundle-x86_64-unknown-linux-gnu.tar.xz`
  alias remains available for ad-hoc download and verification.

## 4. Post-Publish Check

- Verify the Linux install:

```bash
sudo apt-get install -y libgtk-4-dev libadwaita-1-dev libjavascriptcoregtk-6.0-dev libwebkitgtk-6.0-dev
cargo install taskers --locked
taskers
```

- Confirm the published Linux install builds the real app binaries directly and only bootstraps the exact version-matched Ghostty runtime on first launch.
- Confirm `cargo install taskers-cli --bin taskersctl --locked` still works as the standalone helper path.
- Confirm `cargo install taskers --locked` on macOS fails with the Linux-only guidance from the published app package.

For dev-desktop testing against the local checkout after a release pass:

```bash
cargo install --path crates/taskers-app --force
```

That reinstalls the repo-local app into Cargo's bin directory as the real `taskers` package.
