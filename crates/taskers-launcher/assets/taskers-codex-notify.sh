#!/bin/sh
set -eu

payload=${1-}
message=
taskers_ctl=${TASKERS_CTL_PATH:-}

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

if [ -z "$taskers_ctl" ] && command -v taskersctl >/dev/null 2>&1; then
  taskers_ctl=$(command -v taskersctl)
fi

if [ -n "$taskers_ctl" ] && [ -x "$taskers_ctl" ]; then
  "$taskers_ctl" notify --title Codex --body "$message" --agent codex >/dev/null 2>&1 || true
fi
