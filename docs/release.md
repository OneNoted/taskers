# Release Prep

Use this checklist before publishing a new `taskers` release.

## 1. Finalize The Repo State

- Make sure the release work is recorded clearly in `jj`.
- Describe the current change with `jj desc -m "<type>: <summary>"` if needed.
- Split unrelated work into separate changes before publishing.
- Bump the workspace version in `Cargo.toml` and update any internal dependency version pins that still reference the previous release.
- Do not run the publish dry-runs until the version bump is done everywhere. Re-running `cargo publish --dry-run` against an already-published version will verify packaged siblings against crates.io's existing releases instead of your local unpublished changes.

## 2. Refresh Release Assets

- Regenerate the README screenshots:

```bash
bash scripts/capture_demo_screenshots.sh
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
bash scripts/smoke_taskers_ui.sh
bash scripts/smoke_taskers_focus_churn.sh
```

- Build the Linux app bundle that the published launcher expects:

```bash
bash scripts/build_linux_bundle.sh
bash scripts/smoke_linux_release_launcher.sh
```

The output asset name must match:

```text
taskers-linux-bundle-v<version>-<target>.tar.xz
```

- Build the macOS release assets on macOS:

```bash
bash scripts/install_macos_codesign_certificate.sh
bash scripts/generate_macos_project.sh
xcodebuild build \
  -project macos/Taskers.xcodeproj \
  -scheme TaskersMac \
  -configuration Release \
  -derivedDataPath build/macos/DerivedData \
  ARCHS="arm64 x86_64" \
  ONLY_ACTIVE_ARCH=NO \
  CODE_SIGNING_ALLOWED=NO \
  CODE_SIGNING_REQUIRED=NO
bash scripts/sign_macos_app.sh
bash scripts/build_macos_dmg.sh
bash scripts/notarize_macos_dmg.sh dist/Taskers-v<version>-universal2.dmg
```

- Set these env vars before importing the certificate or notarizing:
  - `TASKERS_MACOS_CERTIFICATE_P12_BASE64`
  - `TASKERS_MACOS_CERTIFICATE_PASSWORD`
  - `TASKERS_MACOS_CODESIGN_IDENTITY`
  - `TASKERS_MACOS_NOTARY_APPLE_ID`
  - `TASKERS_MACOS_NOTARY_TEAM_ID`
  - `TASKERS_MACOS_NOTARY_PASSWORD`
- `scripts/sign_macos_app.sh` still falls back to ad hoc signing when `TASKERS_MACOS_CODESIGN_IDENTITY` is unset, but public release builds should always use a Developer ID Application identity and notarize the DMG.

- Build the release manifest from the generated assets:

```bash
python3 scripts/build_release_manifest.py
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

- Push the release tag so GitHub Actions can assemble the assets and attach them to a draft GitHub release.
- Confirm the draft release tagged `v<version>` contains:
  - `taskers-manifest-v<version>.json`
  - `taskers-linux-bundle-v<version>-x86_64-unknown-linux-gnu.tar.xz`
  - `Taskers-v<version>-universal2.dmg`
- Publish the GitHub release so the launcher assets are publicly downloadable before publishing the crates.
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

- Verify the Linux launcher install:

```bash
cargo install taskers --locked
taskers --demo
```

- Confirm the published Linux launcher downloads the exact version-matched bundle on first launch.
- Confirm macOS installs from the published DMG and launches correctly after dragging `Taskers.app` into `Applications`.
- Confirm `cargo install taskers --locked` fails on macOS with guidance to use the GitHub Releases DMG.
- Confirm `cargo install taskers-cli --bin taskersctl --locked` still works as the standalone helper path.
