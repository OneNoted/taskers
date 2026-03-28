#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temp_dir=$(mktemp -d -t taskers-release-install.XXXXXX)
install_root="$temp_dir/install-root"
bin_dir="$install_root/bin"

cleanup() {
  rm -rf "$temp_dir"
}
trap cleanup EXIT

(
  cd "$REPO_ROOT"
  cargo install --path crates/taskers-app --root "$install_root" --force >/dev/null
)

TASKERS_SKIP_DESKTOP_INTEGRATION=1 \
TASKERS_TERMINAL_BACKEND=mock \
  bash "$REPO_ROOT/scripts/headless-smoke.sh" \
    "$bin_dir/taskers" \
    --smoke-script baseline \
    --diagnostic-log stderr \
    --quit-after-ms 5000

test -x "$bin_dir/taskers"
test -x "$bin_dir/taskers-gtk"
test -x "$bin_dir/taskersctl"
test ! -d "$install_root/taskers"
