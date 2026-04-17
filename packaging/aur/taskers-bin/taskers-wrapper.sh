#!/bin/sh
set -eu

root='/opt/taskers'
cmd="$(basename "$0")"

case "$cmd" in
  taskers)
    export TASKERS_CTL_PATH="$root/bin/taskersctl"
    export GHOSTTY_GTK_RUNTIME_DIR="$root/ghostty"
    export TASKERS_GHOSTTY_RUNTIME_DIR="$root/ghostty"
    export GHOSTTY_RESOURCES_DIR="$root/ghostty"
    export TERMINFO="$root/terminfo"
    export GHOSTTY_GTK_DISABLE_RUNTIME_BOOTSTRAP=1
    export TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP=1
    exec "$root/bin/taskers" "$@"
    ;;
  taskersctl)
    exec "$root/bin/taskersctl" "$@"
    ;;
  taskers-terminald)
    export GHOSTTY_GTK_RUNTIME_DIR="$root/ghostty"
    export TASKERS_GHOSTTY_RUNTIME_DIR="$root/ghostty"
    export GHOSTTY_RESOURCES_DIR="$root/ghostty"
    export TERMINFO="$root/terminfo"
    export GHOSTTY_GTK_DISABLE_RUNTIME_BOOTSTRAP=1
    export TASKERS_DISABLE_GHOSTTY_RUNTIME_BOOTSTRAP=1
    exec "$root/bin/taskers-terminald" "$@"
    ;;
  *)
    printf 'unknown Taskers entrypoint: %s\n' "$cmd" >&2
    exit 1
    ;;
esac
