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
  elif command -v python3 >/dev/null 2>&1; then
    message=$(
      python3 - "$payload" <<'PY'
import json
import sys

payload = sys.argv[1] if len(sys.argv) > 1 else ""
message = ""
if payload:
    try:
        decoded = json.loads(payload)
    except Exception:
        decoded = {}
    if isinstance(decoded, dict):
        message = (
            decoded.get("last-assistant-message")
            or decoded.get("message")
            or decoded.get("title")
            or ""
        )
print(str(message)[:160], end="")
PY
    )
  fi
fi

if [ -z "$message" ]; then
  message="Turn complete"
fi

if command -v taskersctl >/dev/null 2>&1; then
  taskersctl notify --title Codex --body "$message" --agent codex >/dev/null 2>&1 || true
fi
