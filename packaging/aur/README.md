# AUR packaging

This directory is the source of truth for Taskers' AUR packages.

Packages:

- `taskers-bin`: installs the latest published GitHub release bundle from the
  stable `releases/latest/download/taskers-linux-bundle-x86_64-unknown-linux-gnu.tar.xz`
  asset. The PKGBUILD derives `pkgver` from the bundle's embedded version marker,
  so new releases do not require a PKGBUILD edit.
- `taskers-git`: builds the current `dev` branch from source and packages the
  same Linux bundle layout under `/opt/taskers`. Until the upstream `dev`
  branch carries the release-bundle build-runtime toggle, this package applies a
  small local patch so the packaged binaries do not retain `$srcdir` runtime
  paths.

## Publish flow

1. Make sure the GitHub release contains both the versioned bundle/manifest
   assets and the stable latest aliases:
   - `taskers-linux-bundle-v<version>-x86_64-unknown-linux-gnu.tar.xz`
   - `taskers-linux-bundle-x86_64-unknown-linux-gnu.tar.xz`
   - `taskers-manifest-v<version>.json`
   - `taskers-manifest.json`
2. Regenerate `.SRCINFO` inside each package directory:

   ```bash
   cd packaging/aur/taskers-bin && makepkg --printsrcinfo > .SRCINFO
   cd packaging/aur/taskers-git && makepkg --printsrcinfo > .SRCINFO
   ```

3. Sync each directory into its matching AUR git repo (`taskers-bin`,
   `taskers-git`) and push.

Both packages install the app bundle into `/opt/taskers` and expose wrappers for
`taskers`, `taskersctl`, and `taskers-terminald` in `/usr/bin`.
