#!/bin/sh
set -eu

if [ "${TASKERS_DISABLE_SHELL_INTEGRATION:-0}" = "1" ]; then
  REAL_SHELL=${TASKERS_REAL_SHELL:-${SHELL:-/bin/sh}}
  SHELL_NAME=${REAL_SHELL##*/}
  SHELL_NAME=${SHELL_NAME#-}

  case "$SHELL_NAME" in
    bash)
      exec "$REAL_SHELL" --noprofile --norc -i "$@"
      ;;
    *)
      exec "$REAL_SHELL" "$@"
      ;;
  esac
fi

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
REAL_SHELL=${TASKERS_REAL_SHELL:-${SHELL:-/bin/sh}}
SHELL_NAME=${REAL_SHELL##*/}
SHELL_NAME=${SHELL_NAME#-}
export TASKERS_EMBEDDED=1
export TERM_PROGRAM=taskers

# Taskers owns shell integration for embedded Ghostty panes. Scrub Ghostty's
# shell-integration environment so user shell config doesn't double-load it.
unset GHOSTTY_BIN_DIR
unset GHOSTTY_RESOURCES_DIR
unset GHOSTTY_SHELL_FEATURES
unset GHOSTTY_SHELL_INTEGRATION_XDG_DIR

case "$SHELL_NAME" in
  bash)
    export TASKERS_USER_BASHRC="${TASKERS_USER_BASHRC:-$HOME/.bashrc}"
    exec "$REAL_SHELL" --rcfile "$SCRIPT_DIR/bash/taskers.bashrc" -i "$@"
    ;;
  *)
    exec "$REAL_SHELL" "$@"
    ;;
esac
