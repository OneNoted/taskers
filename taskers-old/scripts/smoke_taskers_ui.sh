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

choose_http_port() {
  python3 - <<'PY'
import socket

with socket.socket() as sock:
    sock.bind(("127.0.0.1", 0))
    print(sock.getsockname()[1])
PY
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

wait_for_tcp() {
  local port=$1
  local attempts=$((WAIT_TIMEOUT_SECONDS * 10))

  while (( attempts > 0 )); do
    if python3 - <<'PY' "$port"
import socket
import sys

port = int(sys.argv[1])
with socket.socket() as sock:
    sock.settimeout(0.2)
    try:
        sock.connect(("127.0.0.1", port))
    except OSError:
        raise SystemExit(1)
raise SystemExit(0)
PY
    then
      return 0
    fi
    sleep "$POLL_INTERVAL_SECONDS"
    attempts=$((attempts - 1))
  done

  printf 'timed out waiting for localhost:%s\n' "$port" >&2
  return 1
}

wait_for_browser_state() {
  local socket_path=$1
  local status_path=$2
  local integrity_path=$3
  local expected_surface_id=$4
  local expected_url=$5
  local expected_title=$6
  local attempts=$((WAIT_TIMEOUT_SECONDS * 10))

  while (( attempts > 0 )); do
    "$TARGET_DIR/taskersctl" query status --socket "$socket_path" >"$status_path"
    if python3 - <<'PY' "$status_path" "$integrity_path" "$expected_surface_id" "$expected_url" "$expected_title"
import json
import sys

status_path, integrity_path, expected_surface_id, expected_url, expected_title = sys.argv[1:6]
with open(status_path, encoding="utf-8") as handle:
    payload = json.load(handle)
with open(integrity_path, encoding="utf-8") as handle:
    integrity = json.load(handle)

model = payload["response"]["Ok"]["session"]["model"]
active_window_id = model["active_window"]
active_workspace_id = model["windows"][active_window_id]["active_workspace"]
workspace = model["workspaces"][active_workspace_id]
pane = workspace["panes"][workspace["active_pane"]]
surface = pane["surfaces"][expected_surface_id]

ok = (
    pane["active_surface"] == expected_surface_id
    and surface["kind"] == "browser"
    and surface["metadata"].get("url") == expected_url
    and surface["metadata"].get("title") == expected_title
    and integrity.get("active_workspace_surface_id") == expected_surface_id
    and integrity.get("active_displayed_surface_id") == expected_surface_id
    and expected_surface_id in integrity.get("cached_browser_surface_ids", [])
    and expected_surface_id in integrity.get("attached_browser_surface_ids", [])
    and integrity.get("active_browser_uri") == expected_url
)
raise SystemExit(0 if ok else 1)
PY
    then
      return 0
    fi

    sleep "$POLL_INTERVAL_SECONDS"
    attempts=$((attempts - 1))
  done

  printf 'timed out waiting for browser state for surface %s\n' "$expected_surface_id" >&2
  cat "$status_path" >&2 || true
  cat "$integrity_path" >&2 || true
  return 1
}

cleanup() {
  local status=$?
  if [[ -n "${app_pid:-}" ]]; then
    kill "$app_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n "${http_pid:-}" ]]; then
    kill "$http_pid" >/dev/null 2>&1 || true
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

temp_dir=$(mktemp -d -t taskers-ui-smoke.XXXXXX)
display_number=$(choose_display_number)
display=":${display_number}"
socket_path="$temp_dir/taskers.sock"
session_path="$temp_dir/session.json"
integrity_path="$temp_dir/ui-integrity.json"
status_path="$temp_dir/status.json"
browser_split_path="$temp_dir/browser-split.json"
site_dir="$temp_dir/site"
http_port=$(choose_http_port)
browser_url="http://127.0.0.1:${http_port}/index.html"

mkdir -p "$site_dir"
cat >"$site_dir/index.html" <<'HTML'
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <title>Taskers Browser Smoke</title>
  </head>
  <body>
    <main>browser smoke page</main>
  </body>
</html>
HTML

(
  cd "$REPO_ROOT"
  cargo build -p taskers-gtk -p taskers-cli
) >/dev/null

python3 -m http.server "$http_port" --bind 127.0.0.1 --directory "$site_dir" \
  >"$temp_dir/http.log" 2>&1 &
http_pid=$!
wait_for_tcp "$http_port"

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
  exec "$TARGET_DIR/taskers-gtk" \
    --demo \
    --socket "$socket_path" \
    --session "$session_path"
) >"$temp_dir/app.log" 2>&1 &
app_pid=$!

wait_for_path "$socket_path"
wait_for_path "$session_path"
wait_for_path "$integrity_path"

sleep 5
kill -0 "$app_pid"

"$TARGET_DIR/taskersctl" query status --socket "$socket_path" >"$status_path"
readarray -t ids < <(
  python3 - <<'PY' "$status_path"
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

model = payload["response"]["Ok"]["session"]["model"]
active_window_id = model["active_window"]
active_workspace_id = model["windows"][active_window_id]["active_workspace"]
workspace = model["workspaces"][active_workspace_id]

print(active_workspace_id)
print(workspace["active_pane"])
PY
)

workspace_id=${ids[0]}
pane_id=${ids[1]}

"$TARGET_DIR/taskersctl" pane split \
  --socket "$socket_path" \
  --workspace "$workspace_id" \
  --pane "$pane_id" \
  --axis horizontal \
  --kind browser \
  --url "$browser_url" >"$browser_split_path"

browser_surface_id=$(
  python3 - <<'PY' "$browser_split_path"
import json
import sys

with open(sys.argv[1], encoding="utf-8") as handle:
    payload = json.load(handle)

print(payload["surface_id"])
PY
)

wait_for_browser_state \
  "$socket_path" \
  "$status_path" \
  "$integrity_path" \
  "$browser_surface_id" \
  "$browser_url" \
  "Taskers Browser Smoke"

kill -0 "$app_pid"

printf '%s\n' 'Taskers UI smoke passed: embedded browser split loaded and synced metadata.'
