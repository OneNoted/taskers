#!/bin/sh
set -eu

INVOKED_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
SCRIPT_PATH=$(readlink -f -- "$0" 2>/dev/null || realpath "$0" 2>/dev/null || printf '%s' "$0")
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$SCRIPT_PATH")" && pwd)
PROXY_PATH="$SCRIPT_DIR/taskers-agent-proxy.sh"
HOOK_SCRIPT="$SCRIPT_DIR/taskers-claude-hook.sh"
INVOKED_NAME=$(basename -- "$0")

proxy_target=$INVOKED_NAME
case "$proxy_target" in
  taskers-agent-claude.sh) proxy_target=claude ;;
esac

json_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

shell_single_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\"'\"'/g")"
}

hook_command() {
  printf '%s %s' "$(shell_single_quote "$HOOK_SCRIPT")" "$1"
}

if [ "${TASKERS_CLAUDE_HOOKS_DISABLED:-0}" = "1" ]; then
  exec env TASKERS_AGENT_PROXY_TARGET="$proxy_target" TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" "$@"
fi

user_prompt_submit_command=$(json_escape "$(hook_command user-prompt-submit)")
notification_command=$(json_escape "$(hook_command notification)")
stop_command=$(json_escape "$(hook_command stop)")
hooks_json=$(cat <<EOF
{"hooks":{"UserPromptSubmit":[{"matcher":"","hooks":[{"type":"command","command":"$user_prompt_submit_command","timeout":10}]}],"Notification":[{"matcher":"permission_prompt","hooks":[{"type":"command","command":"$notification_command","timeout":10}]},{"matcher":"idle_prompt","hooks":[{"type":"command","command":"$notification_command","timeout":10}]},{"matcher":"elicitation_dialog","hooks":[{"type":"command","command":"$notification_command","timeout":10}]}],"Stop":[{"matcher":"","hooks":[{"type":"command","command":"$stop_command","timeout":10}]}]}}
EOF
)

exec env TASKERS_AGENT_PROXY_TARGET="$proxy_target" TASKERS_AGENT_PROXY_SHIM_DIR="$INVOKED_DIR" "$PROXY_PATH" --settings "$hooks_json" "$@"
