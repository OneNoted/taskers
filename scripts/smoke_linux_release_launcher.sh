#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET_DIR="$REPO_ROOT/target/debug"
temp_dir=$(mktemp -d -t taskers-release-launcher.XXXXXX)
install_root="$temp_dir/install"
manifest_path="$temp_dir/taskers-manifest.json"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
target="$(rustc -vV | sed -n 's/^host: //p')"

cleanup() {
  rm -rf "$temp_dir"
}
trap cleanup EXIT

(
  cd "$REPO_ROOT"
  cargo build -p taskers --bin taskers >/dev/null
  python3 scripts/build_release_manifest.py \
    --dist-dir "$REPO_ROOT/dist" \
    --base-url "$REPO_ROOT/dist" \
    --output "$manifest_path" >/dev/null
)

TASKERS_INSTALL_ROOT="$install_root" \
TASKERS_RELEASE_MANIFEST_URL="$manifest_path" \
TASKERS_SKIP_DESKTOP_INTEGRATION=1 \
TASKERS_TERMINAL_BACKEND=mock \
  bash "$REPO_ROOT/scripts/headless-smoke.sh" \
    "$TARGET_DIR/taskers" \
    --smoke-script baseline \
    --diagnostic-log stderr \
    --quit-after-ms 5000

test -x "$install_root/$version/$target/bin/taskers"
test -x "$install_root/$version/$target/bin/taskersctl"
