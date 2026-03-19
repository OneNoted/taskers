#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <binary-path> [args...]" >&2
  exit 2
fi

timeout "${TIMEOUT_SECONDS:-8}" \
  dbus-run-session -- \
  env LIBGL_ALWAYS_SOFTWARE=1 \
  xvfb-run -a \
  "$@"
