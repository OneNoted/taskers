#!/bin/sh
set -eu

INVOKED_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || realpath "$0" 2>/dev/null || printf '%s' "$0")
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd)
PROXY_PATH="$SCRIPT_DIR/taskers-agent-proxy.sh"
NOTIFY_SCRIPT="$SCRIPT_DIR/taskers-codex-notify.sh"

toml_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

if [ "${TASKERS_CODEX_HOOKS_DISABLED:-0}" = "1" ]; then
  exec env TASKERS_AGENT_PROXY_TARGET=codex TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" "$@"
fi

notify_override=$(printf 'notify=["bash","%s"]' "$(toml_escape "$NOTIFY_SCRIPT")")

exec env TASKERS_AGENT_PROXY_TARGET=codex TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" -c "$notify_override" "$@"
