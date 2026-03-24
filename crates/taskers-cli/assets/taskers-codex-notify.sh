#!/bin/sh
set -eu

payload=${1-}
message=
taskers_ctl=${TASKERS_CTL_PATH:-}

if [ -z "$taskers_ctl" ] && command -v taskersctl >/dev/null 2>&1; then
  taskers_ctl=$(command -v taskersctl)
fi

if [ -n "$payload" ]; then
  if command -v jq >/dev/null 2>&1; then
    message=$(
      printf '%s' "$payload" \
        | jq -r '."last-assistant-message" // .message // .title // empty' 2>/dev/null \
        | head -c 160
    )
  fi
fi

if [ -z "$message" ]; then
  message="Turn complete"
fi

has_embedded_surface_context() {
  [ -x "${taskers_ctl:-}" ] || return 1
  [ -n "${TASKERS_WORKSPACE_ID:-}" ] || return 1
  [ -n "${TASKERS_PANE_ID:-}" ] || return 1
  [ -n "${TASKERS_SURFACE_ID:-}" ] || return 1
  [ -n "${TASKERS_TTY_NAME:-}" ] || return 1

  current_tty=$(tty 2>/dev/null || true)
  case "$current_tty" in
    /dev/*) ;;
    *) return 1 ;;
  esac

  [ "$current_tty" = "$TASKERS_TTY_NAME" ] || return 1
}

if has_embedded_surface_context; then
  "$taskers_ctl" agent-hook notification \
    --workspace "$TASKERS_WORKSPACE_ID" \
    --pane "$TASKERS_PANE_ID" \
    --surface "$TASKERS_SURFACE_ID" \
    --agent codex \
    --title Codex \
    --message "$message" >/dev/null 2>&1 || true
fi
