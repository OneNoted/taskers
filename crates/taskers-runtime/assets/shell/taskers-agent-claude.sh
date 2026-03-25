#!/bin/sh
set -eu

INVOKED_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || realpath "$0" 2>/dev/null || printf '%s' "$0")
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd)
PROXY_PATH="$SCRIPT_DIR/taskers-agent-proxy.sh"
HOOK_SCRIPT="$SCRIPT_DIR/taskers-claude-hook.sh"

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

if [ "${TASKERS_CLAUDE_HOOKS_DISABLED:-0}" = "1" ]; then
  exec env TASKERS_AGENT_PROXY_TARGET=claude TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" "$@"
fi

hook_script_escaped=$(json_escape "$HOOK_SCRIPT")
hooks_json=$(cat <<EOF
{"hooks":{"UserPromptSubmit":[{"matcher":"","hooks":[{"type":"command","command":"$hook_script_escaped user-prompt-submit","timeout":10}]}],"Notification":[{"matcher":"permission_prompt","hooks":[{"type":"command","command":"$hook_script_escaped notification","timeout":10}]},{"matcher":"idle_prompt","hooks":[{"type":"command","command":"$hook_script_escaped notification","timeout":10}]},{"matcher":"elicitation_dialog","hooks":[{"type":"command","command":"$hook_script_escaped notification","timeout":10}]}],"Stop":[{"matcher":"","hooks":[{"type":"command","command":"$hook_script_escaped stop","timeout":10}]}]}}
EOF
)

exec env TASKERS_AGENT_PROXY_TARGET=claude TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" --settings "$hooks_json" "$@"
