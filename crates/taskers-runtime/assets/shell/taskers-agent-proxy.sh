#!/bin/sh
set -eu

proxy_name=${TASKERS_AGENT_PROXY_TARGET:-$(basename -- "$0")}
proxy_invoked_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
proxy_script_path=$(readlink -f -- "$0" 2>/dev/null || realpath "$0" 2>/dev/null || printf '%s' "$0")
proxy_dir=$(CDPATH= cd -- "$(dirname -- "$proxy_script_path")" && pwd)
shim_dir=${TASKERS_AGENT_PROXY_SHIM_DIR:-$proxy_invoked_dir}

agent_kind=$proxy_name
case "$proxy_name" in
  claude-code) agent_kind=claude ;;
esac

filtered_path=
old_ifs=${IFS:-" "}
IFS=:
for entry in ${PATH:-}; do
  if [ "$entry" = "$proxy_dir" ] || [ "$entry" = "$shim_dir" ] || [ -z "$entry" ]; then
    continue
  fi
  if [ -n "$filtered_path" ]; then
    filtered_path="${filtered_path}:$entry"
  else
    filtered_path="$entry"
  fi
done
IFS=$old_ifs

if [ -z "$filtered_path" ]; then
  filtered_path=${PATH:-}
fi

real_binary=$(PATH="$filtered_path" command -v -- "$proxy_name" || true)
if [ -z "$real_binary" ]; then
  printf '%s: failed to locate real command in PATH\n' "$proxy_name" >&2
  exit 127
fi

context_tty_matches() {
  expected_tty=${TASKERS_TTY_NAME:-}
  [ -n "$expected_tty" ] || return 1
  current_tty=$(tty 2>/dev/null || true)
  case "$current_tty" in
    /dev/*) ;;
    *) return 1 ;;
  esac
  [ "$current_tty" = "$expected_tty" ]
}

can_emit_lifecycle() {
  [ -x "${TASKERS_CTL_PATH:-}" ] || return 1
  [ -n "${TASKERS_WORKSPACE_ID:-}" ] || return 1
  [ -n "${TASKERS_PANE_ID:-}" ] || return 1
  [ -n "${TASKERS_SURFACE_ID:-}" ] || return 1
  context_tty_matches
}

emit_surface_agent_start() {
  can_emit_lifecycle || return 1
  "$TASKERS_CTL_PATH" surface agent-start \
    --workspace "$TASKERS_WORKSPACE_ID" \
    --pane "$TASKERS_PANE_ID" \
    --surface "$TASKERS_SURFACE_ID" \
    --agent "$agent_kind" >/dev/null 2>&1 || true
  return 0
}

emit_surface_agent_stop() {
  status=$1
  can_emit_lifecycle || return 1
  "$TASKERS_CTL_PATH" surface agent-stop \
    --workspace "$TASKERS_WORKSPACE_ID" \
    --pane "$TASKERS_PANE_ID" \
    --surface "$TASKERS_SURFACE_ID" \
    --exit-status "$status" >/dev/null 2>&1 || true
  return 0
}

started_lifecycle=0
if emit_surface_agent_start; then
  started_lifecycle=1
fi

set +e
TASKERS_AGENT_PROXY_ACTIVE=1 PATH="$filtered_path" "$real_binary" "$@"
status=$?
set -e

if [ "$started_lifecycle" = 1 ]; then
  emit_surface_agent_stop "$status" || true
fi

exit "$status"
