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

wait_for_integrity_state() {
  local integrity_path=$1
  local expected_pane_id=$2
  local expected_surface_id=$3
  local attempts=$((WAIT_TIMEOUT_SECONDS * 10))

  while (( attempts > 0 )); do
    if python3 - <<'PY' "$integrity_path" "$expected_pane_id" "$expected_surface_id"
import json
import sys

integrity_path, expected_pane_id, expected_surface_id = sys.argv[1:4]
with open(integrity_path, encoding="utf-8") as handle:
    data = json.load(handle)

ok = (
    data.get("active_workspace_pane_id") == expected_pane_id
    and data.get("active_workspace_surface_id") == expected_surface_id
    and data.get("active_displayed_surface_id") == expected_surface_id
    and data.get("active_terminal_child_count") == 1
    and data.get("active_pane_focus_has_focus") is True
)
raise SystemExit(0 if ok else 1)
PY
    then
      return 0
    fi

    sleep "$POLL_INTERVAL_SECONDS"
    attempts=$((attempts - 1))
  done

  printf 'timed out waiting for focus churn integrity state in %s\n' "$integrity_path" >&2
  python3 - <<'PY' "$integrity_path" >&2 || true
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    data = json.load(handle)

for key in [
    "active_workspace_pane_id",
    "active_workspace_surface_id",
    "active_displayed_surface_id",
    "active_terminal_child_count",
    "active_pane_focus_has_focus",
]:
    print(f"{key}={data.get(key)!r}")
PY
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
  printf '%s\n' 'Xvfb is required for the UI smoke test.' >&2
  exit 1
fi

temp_dir=$(mktemp -d -t taskers-focus-smoke.XXXXXX)
display_number=$(choose_display_number)
display=":${display_number}"
socket_path="$temp_dir/taskers.sock"
session_path="$temp_dir/session.json"
integrity_path="$temp_dir/ui-integrity.json"
status_path="$temp_dir/status.json"

(
  cd "$REPO_ROOT"
  cargo build -p taskers -p taskers-cli
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
  export TASKERS_NON_UNIQUE=1
  export TASKERS_TERMINAL_BACKEND=mock
  export TASKERS_UI_INTEGRITY_PATH="$integrity_path"
  exec "$TARGET_DIR/taskers" \
    --demo \
    --socket "$socket_path" \
    --session "$session_path"
) >"$temp_dir/app.log" 2>&1 &
app_pid=$!

wait_for_path "$socket_path"
wait_for_path "$session_path"
wait_for_path "$integrity_path"

"$TARGET_DIR/taskersctl" query status --socket "$socket_path" >"$status_path"

readarray -t ids < <(
  python3 - <<'PY' "$status_path"
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

response = payload["response"]["Ok"]
session = response["session"]
model = session["model"]
active_window_id = model["active_window"]
active_workspace_id = model["windows"][active_window_id]["active_workspace"]
workspace = model["workspaces"][active_workspace_id]
active_pane_id = workspace["active_pane"]
active_surface_id = workspace["panes"][active_pane_id]["active_surface"]

print(active_workspace_id)
print(active_pane_id)
print(active_surface_id)
PY
)

workspace_id=${ids[0]}
pane_id=${ids[1]}
surface_id=${ids[2]}

wait_for_integrity_state "$integrity_path" "$pane_id" "$surface_id"

for iteration in $(seq 1 8); do
  "$TARGET_DIR/taskersctl" pane update \
    --socket "$socket_path" \
    --pane "$pane_id" \
    --title "Focus churn $iteration" \
    --cwd "$temp_dir/cwd-$iteration" >/dev/null
  wait_for_integrity_state "$integrity_path" "$pane_id" "$surface_id"
done

"$TARGET_DIR/taskersctl" query status --socket "$socket_path" >"$status_path"
python3 - <<'PY' "$status_path" "$workspace_id" "$pane_id" "$surface_id"
import json
import sys

status_path, workspace_id, pane_id, surface_id = sys.argv[1:5]
with open(status_path, encoding="utf-8") as handle:
    payload = json.load(handle)

response = payload["response"]["Ok"]
session = response["session"]
model = session["model"]
active_window_id = model["active_window"]
active_workspace_id = model["windows"][active_window_id]["active_workspace"]
workspace = model["workspaces"][active_workspace_id]

assert active_workspace_id == workspace_id
assert workspace["active_pane"] == pane_id
assert workspace["panes"][pane_id]["active_surface"] == surface_id
PY

printf '%s\n' 'Taskers focus churn smoke passed: metadata updates preserved focus and terminal mount state.'
