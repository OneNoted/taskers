#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET_DIR="$REPO_ROOT/target/debug"
OUT_DIR="${1:-$REPO_ROOT/docs/screenshots}"
DISPLAY_SIZE="1440x960"
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

current_workspace_and_pane() {
  local socket_path=$1
  python3 - <<'PY' "$socket_path"
import json
import subprocess
import sys

socket_path = sys.argv[1]
payload = subprocess.check_output(
    ["target/debug/taskersctl", "query", "status", "--socket", socket_path],
    text=True,
)
status = json.loads(payload)
model = status["response"]["Ok"]["session"]["model"]
active_window_id = model["active_window"]
workspace_id = model["windows"][active_window_id]["active_workspace"]
workspace = model["workspaces"][workspace_id]
print(workspace_id)
print(workspace["active_pane"])
PY
}

workspace_and_pane_by_label() {
  local socket_path=$1
  local label=$2
  python3 - <<'PY' "$socket_path" "$label"
import json
import subprocess
import sys

socket_path, label = sys.argv[1:3]
payload = subprocess.check_output(
    ["target/debug/taskersctl", "query", "status", "--socket", socket_path],
    text=True,
)
status = json.loads(payload)
model = status["response"]["Ok"]["session"]["model"]
for workspace_id, workspace in model["workspaces"].items():
    if workspace["label"] == label:
        print(workspace_id)
        print(workspace["active_pane"])
        break
else:
    raise SystemExit(f"workspace {label!r} not found")
PY
}

capture_display() {
  local out_path=$1
  ffmpeg -y -loglevel error -draw_mouse 0 \
    -f x11grab \
    -video_size "$DISPLAY_SIZE" \
    -i "$DISPLAY" \
    -frames:v 1 \
    "$out_path"
}

cleanup_scene() {
  if [[ -n "${app_pid:-}" ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
    wait "$app_pid" >/dev/null 2>&1 || true
    unset app_pid
  fi
  if [[ -n "${xvfb_pid:-}" ]]; then
    kill "$xvfb_pid" >/dev/null 2>&1 || true
    wait "$xvfb_pid" >/dev/null 2>&1 || true
    unset xvfb_pid
  fi
  if [[ -n "${temp_dir:-}" ]] && [[ -d "$temp_dir" ]]; then
    rm -rf "$temp_dir"
    unset temp_dir
  fi
}

start_scene() {
  temp_dir=$(mktemp -d -t taskers-screenshots.XXXXXX)
  display_number=$(choose_display_number)
  DISPLAY=":${display_number}"
  export DISPLAY

  socket_path="$temp_dir/taskers.sock"
  session_path="$temp_dir/session.json"

  Xvfb "$DISPLAY" -screen 0 "${DISPLAY_SIZE}x24" >"$temp_dir/xvfb.log" 2>&1 &
  xvfb_pid=$!
  wait_for_path "/tmp/.X11-unix/X${display_number}"

  mkdir -p "$temp_dir/config" "$temp_dir/data" "$temp_dir/cache" "$temp_dir/home"
  (
    cd "$REPO_ROOT"
    export DISPLAY
    export GDK_BACKEND=x11
    export GSK_RENDERER=cairo
    export LIBGL_ALWAYS_SOFTWARE=1
    export TASKERS_NON_UNIQUE=1
    export TASKERS_TERMINAL_BACKEND=mock
    export SHELL=/bin/bash
    export XDG_CONFIG_HOME="$temp_dir/config"
    export XDG_DATA_HOME="$temp_dir/data"
    export XDG_CACHE_HOME="$temp_dir/cache"
    export HOME="$temp_dir/home"
    exec "$TARGET_DIR/taskers-gtk" \
      --raw-shell \
      --socket "$socket_path" \
      --session "$session_path"
  ) >"$temp_dir/app.log" 2>&1 &
  app_pid=$!

  wait_for_path "$socket_path"
  sleep 8
}

build_base_scene() {
  readarray -t repo_ids < <(current_workspace_and_pane "$socket_path")
  repo_workspace_id=${repo_ids[0]}
  repo_pane_id=${repo_ids[1]}

  "$TARGET_DIR/taskersctl" workspace rename \
    --socket "$socket_path" \
    --workspace "$repo_workspace_id" \
    --label "Repo A" >/dev/null
  "$TARGET_DIR/taskersctl" pane update \
    --socket "$socket_path" \
    --pane "$repo_pane_id" \
    --title "Codex" \
    --cwd /home/notes/Projects/taskers \
    --repo taskers \
    --branch main \
    --agent codex >/dev/null
  "$TARGET_DIR/taskersctl" signal \
    --socket "$socket_path" \
    --workspace "$repo_workspace_id" \
    --pane "$repo_pane_id" \
    --kind waiting-input \
    --message "Ready for release review" \
    --title "Codex" \
    --cwd /home/notes/Projects/taskers \
    --repo taskers \
    --branch main \
    --agent codex >/dev/null

  "$TARGET_DIR/taskersctl" workspace new \
    --socket "$socket_path" \
    --label "Docs" >/dev/null
  sleep 1

  readarray -t docs_ids < <(workspace_and_pane_by_label "$socket_path" "Docs")
  docs_workspace_id=${docs_ids[0]}
  docs_pane_id=${docs_ids[1]}

  "$TARGET_DIR/taskersctl" pane update \
    --socket "$socket_path" \
    --pane "$docs_pane_id" \
    --title "Release Notes" \
    --cwd /home/notes/Documents/release-notes \
    --repo notes \
    --branch docs/release \
    --agent opencode >/dev/null
  "$TARGET_DIR/taskersctl" signal \
    --socket "$socket_path" \
    --workspace "$docs_workspace_id" \
    --pane "$docs_pane_id" \
    --kind completed \
    --message "Draft release notes completed" \
    --title "Release Notes" \
    --cwd /home/notes/Documents/release-notes \
    --repo notes \
    --branch docs/release \
    --agent opencode >/dev/null
}

capture_attention_scene() {
  start_scene
  build_base_scene
  "$TARGET_DIR/taskersctl" workspace switch \
    --socket "$socket_path" \
    --workspace "$docs_workspace_id" >/dev/null
  sleep 2
  capture_display "$OUT_DIR/demo-attention.png"
  cleanup_scene
}

capture_layout_scene() {
  start_scene
  build_base_scene
  "$TARGET_DIR/taskersctl" workspace switch \
    --socket "$socket_path" \
    --workspace "$repo_workspace_id" >/dev/null
  "$TARGET_DIR/taskersctl" pane split \
    --socket "$socket_path" \
    --workspace "$repo_workspace_id" \
    --pane "$repo_pane_id" \
    --axis vertical >/dev/null
  sleep 1

  readarray -t layout_ids < <(workspace_and_pane_by_label "$socket_path" "Repo A")
  split_pane_id=${layout_ids[1]}

  "$TARGET_DIR/taskersctl" pane update \
    --socket "$socket_path" \
    --pane "$split_pane_id" \
    --title "Tests" \
    --cwd /home/notes/Projects/taskers \
    --repo taskers \
    --branch release/prep \
    --agent claude >/dev/null
  "$TARGET_DIR/taskersctl" signal \
    --socket "$socket_path" \
    --workspace "$repo_workspace_id" \
    --pane "$split_pane_id" \
    --kind started \
    --message "Running release checks" \
    --title "Tests" \
    --cwd /home/notes/Projects/taskers \
    --repo taskers \
    --branch release/prep \
    --agent claude >/dev/null
  sleep 2

  capture_display "$OUT_DIR/demo-layout.png"
  cleanup_scene
}

cleanup() {
  local status=$?
  cleanup_scene
  exit "$status"
}

trap cleanup EXIT

for cmd in Xvfb ffmpeg python3 cargo; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    printf 'missing required command: %s\n' "$cmd" >&2
    exit 1
  fi
done

mkdir -p "$OUT_DIR"

(
  cd "$REPO_ROOT"
  cargo build -p taskers-gtk -p taskers-cli >/dev/null
)

capture_attention_scene
capture_layout_scene

printf '%s\n' "$OUT_DIR/demo-attention.png"
printf '%s\n' "$OUT_DIR/demo-layout.png"
