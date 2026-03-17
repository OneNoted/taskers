#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET_DIR="$REPO_ROOT/target/debug"
POLL_INTERVAL_SECONDS=0.1
WAIT_TIMEOUT_SECONDS=30

choose_display_number() {
  local display_number
  for display_number in $(seq 99 119); do
    if [[ ! -e "/tmp/.X11-unix/X${display_number}" ]]; then
      printf '%s\n' "$display_number"
      return 0
    fi
  done

  printf '%s\n' 'could not find a free X display number between :99 and :119' >&2
  return 1
}

wait_for_path() {
  local path=$1
  local attempts=$((WAIT_TIMEOUT_SECONDS * 10))

  while (( attempts > 0 )); do
    if [[ -e "$path" ]]; then
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
    attempts=$((attempts - 1))
  done

  printf 'timed out waiting for %s\n' "$path" >&2
  return 1
}

cleanup() {
  local status=$?
  if [[ -n "${app_pid:-}" ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n "${xvfb_pid:-}" ]]; then
    kill "$xvfb_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n "${temp_dir:-}" ]] && [[ -d "$temp_dir" ]]; then
    rm -rf "$temp_dir"
  fi
  exit "$status"
}

trap cleanup EXIT

if ! command -v Xvfb >/dev/null 2>&1; then
  printf '%s\n' 'Xvfb is required for the launcher smoke test.' >&2
  exit 1
fi

temp_dir=$(mktemp -d -t taskers-release-launcher.XXXXXX)
display_number=$(choose_display_number)
display=":${display_number}"
socket_path="$temp_dir/taskers.sock"
session_path="$temp_dir/session.json"
install_root="$temp_dir/install"
xdg_data_home="$temp_dir/data"
xdg_bin_home="$temp_dir/bin"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
target="$(rustc -vV | sed -n 's/^host: //p')"
manifest_path="$temp_dir/taskers-manifest-v${version}.json"
bundle_taskersctl="$install_root/$version/$target/bin/taskersctl"

(
  cd "$REPO_ROOT"
  cargo build -p taskers --bin taskers
  python3 scripts/build_release_manifest.py \
    --dist-dir "$REPO_ROOT/dist" \
    --base-url "$REPO_ROOT/dist" \
    --output "$manifest_path"
) >/dev/null

Xvfb "$display" -screen 0 1440x960x24 >"$temp_dir/xvfb.log" 2>&1 &
xvfb_pid=$!
wait_for_path "/tmp/.X11-unix/X${display_number}"

(
  cd "$REPO_ROOT"
  export DISPLAY="$display"
  export GDK_BACKEND=x11
  export GSK_RENDERER=cairo
  export LIBGL_ALWAYS_SOFTWARE=1
  export TASKERS_INSTALL_ROOT="$install_root"
  export TASKERS_NON_UNIQUE=1
  export TASKERS_RELEASE_MANIFEST_URL="$manifest_path"
  export TASKERS_SKIP_DESKTOP_INTEGRATION=1
  export TASKERS_TERMINAL_BACKEND=mock
  export XDG_BIN_HOME="$xdg_bin_home"
  export XDG_DATA_HOME="$xdg_data_home"
  exec "$TARGET_DIR/taskers" \
    --demo \
    --socket "$socket_path" \
    --session "$session_path"
) >"$temp_dir/app.log" 2>&1 &
app_pid=$!

wait_for_path "$socket_path"
wait_for_path "$session_path"

if [[ ! -x "$bundle_taskersctl" ]]; then
  printf 'expected bundled taskersctl at %s\n' "$bundle_taskersctl" >&2
  exit 1
fi

sleep 5
kill -0 "$app_pid"

"$bundle_taskersctl" workspace new --label "Release Smoke" --socket "$socket_path" >/dev/null
sleep 1
kill -0 "$app_pid"

printf '%s\n' 'Taskers launcher smoke passed: release bundle installed and responded to control commands.'
