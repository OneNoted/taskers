#!/bin/sh
set -eu

hook_name=${1-}
payload=
taskers_ctl=${TASKERS_CTL_PATH:-}
message=
notification_type=

if [ -z "$hook_name" ]; then
  exit 64
fi

if [ -z "$taskers_ctl" ] && command -v taskersctl >/dev/null 2>&1; then
  taskers_ctl=$(command -v taskersctl)
fi

if [ ! -x "${taskers_ctl:-}" ]; then
  exit 0
fi

if [ ! -t 0 ]; then
  payload=$(cat || true)
fi

if [ -n "$payload" ] && command -v jq >/dev/null 2>&1; then
  case "$hook_name" in
    notification)
      notification_type=$(
        printf '%s' "$payload" \
          | jq -r '.notification_type // empty' 2>/dev/null \
          | head -c 64
      )
      message=$(
        printf '%s' "$payload" \
          | jq -r '.message // .title // empty' 2>/dev/null \
          | head -c 160
      )
      ;;
    stop)
      message=$(
        printf '%s' "$payload" \
          | jq -r '.last_assistant_message // empty' 2>/dev/null \
          | head -c 160
      )
      ;;
  esac
fi

has_embedded_surface_context() {
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

if ! has_embedded_surface_context; then
  exit 0
fi

run_hook() {
  subcommand=$1
  shift
  "$taskers_ctl" agent-hook "$subcommand" \
    --workspace "$TASKERS_WORKSPACE_ID" \
    --pane "$TASKERS_PANE_ID" \
    --surface "$TASKERS_SURFACE_ID" \
    --agent claude \
    --title Claude "$@" >/dev/null 2>&1 || true
}

case "$hook_name" in
  user-prompt-submit)
    run_hook progress
    ;;
  notification)
    case "$notification_type" in
      permission_prompt|idle_prompt|elicitation_dialog)
        if [ -n "$message" ]; then
          run_hook waiting --message "$message"
        else
          run_hook waiting
        fi
        ;;
      *)
        exit 0
        ;;
    esac
    ;;
  stop)
    if [ -n "$message" ]; then
      run_hook stop --message "$message"
    else
      run_hook stop
    fi
    ;;
  *)
    exit 0
    ;;
esac
