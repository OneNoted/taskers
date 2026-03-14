#!/bin/sh
set -eu

payload=${1-}
message=

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

if command -v taskersctl >/dev/null 2>&1; then
  taskersctl notify --title Codex --body "$message" --agent codex >/dev/null 2>&1 || true
fi
