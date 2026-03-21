#!/usr/bin/env bash
set -euo pipefail

dmg_path="${1:-}"

if [[ -z "$dmg_path" ]]; then
  echo "usage: $0 <path-to-dmg>" >&2
  exit 1
fi

if [[ ! -f "$dmg_path" ]]; then
  echo "expected dmg at $dmg_path" >&2
  exit 1
fi

if [[ -z "${TASKERS_MACOS_NOTARY_APPLE_ID:-}" ]]; then
  echo "TASKERS_MACOS_NOTARY_APPLE_ID is required" >&2
  exit 1
fi

if [[ -z "${TASKERS_MACOS_NOTARY_TEAM_ID:-}" ]]; then
  echo "TASKERS_MACOS_NOTARY_TEAM_ID is required" >&2
  exit 1
fi

if [[ -z "${TASKERS_MACOS_NOTARY_PASSWORD:-}" ]]; then
  echo "TASKERS_MACOS_NOTARY_PASSWORD is required" >&2
  exit 1
fi

xcrun notarytool submit "$dmg_path" \
  --wait \
  --apple-id "$TASKERS_MACOS_NOTARY_APPLE_ID" \
  --team-id "$TASKERS_MACOS_NOTARY_TEAM_ID" \
  --password "$TASKERS_MACOS_NOTARY_PASSWORD"
xcrun stapler staple "$dmg_path"
xcrun stapler validate "$dmg_path"
spctl --assess --type open --context context:primary-signature --verbose=4 "$dmg_path"

printf '%s\n' "$dmg_path"
