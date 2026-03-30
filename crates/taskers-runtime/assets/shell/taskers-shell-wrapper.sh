#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REAL_SHELL=${TASKERS_REAL_SHELL:-${SHELL:-/bin/sh}}
SHELL_NAME=${REAL_SHELL##*/}
SHELL_NAME=${SHELL_NAME#-}
export TASKERS_EMBEDDED=1
export TERM_PROGRAM=taskers
current_tty=$(tty 2>/dev/null || true)
case "$current_tty" in
  /dev/*) export TASKERS_TTY_NAME="$current_tty" ;;
esac

# Taskers owns shell integration for embedded Ghostty panes. Scrub Ghostty's
# shell-integration environment so user shell config doesn't double-load it.
unset GHOSTTY_BIN_DIR
unset GHOSTTY_RESOURCES_DIR
unset GHOSTTY_SHELL_FEATURES
unset GHOSTTY_SHELL_INTEGRATION_XDG_DIR

if [ "${TASKERS_TMUX_CHILD:-0}" = "1" ]; then
  unset TASKERS_TMUX_CHILD
elif [ -n "${TASKERS_TMUX_SOCKET:-}" ] && [ -n "${TASKERS_TERMINAL_SESSION_ID:-}" ]; then
  taskers_ctl=${TASKERS_CTL_PATH:-}
  if [ -z "$taskers_ctl" ] && command -v taskersctl >/dev/null 2>&1; then
    taskers_ctl=$(command -v taskersctl)
  fi
  if [ -z "$taskers_ctl" ] || [ ! -x "$taskers_ctl" ]; then
    echo "taskers shell wrapper: taskersctl is required for tmux attach" >&2
    exit 127
  fi
  exec "$taskers_ctl" tmux attach \
    --socket "$TASKERS_TMUX_SOCKET" \
    --session "$TASKERS_TERMINAL_SESSION_ID" \
    -- "$@"
fi

if [ "${TASKERS_DISABLE_SHELL_INTEGRATION:-0}" = "1" ]; then
  case "$SHELL_NAME" in
    bash)
      exec "$REAL_SHELL" --noprofile --norc -i "$@"
      ;;
    *)
      exec "$REAL_SHELL" "$@"
      ;;
  esac
fi

case "$SHELL_NAME" in
  bash)
    export TASKERS_USER_BASHRC="${TASKERS_USER_BASHRC:-$HOME/.bashrc}"
    exec "$REAL_SHELL" --rcfile "$SCRIPT_DIR/bash/taskers.bashrc" -i "$@"
    ;;
  *)
    exec "$REAL_SHELL" "$@"
    ;;
esac
