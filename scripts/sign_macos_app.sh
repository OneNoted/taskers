#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
derived_data_path="${1:-$repo_root/build/macos/DerivedData}"
app_path="${2:-$derived_data_path/Build/Products/Release/Taskers.app}"
identity="${TASKERS_MACOS_CODESIGN_IDENTITY:--}"

if [[ ! -d "$app_path" ]]; then
  echo "expected Taskers.app at $app_path" >&2
  exit 1
fi

codesign_args=(
  --force
  --deep
  --sign "$identity"
)

if [[ "$identity" != "-" ]]; then
  codesign_args+=(
    --options runtime
    --timestamp
  )
fi

codesign "${codesign_args[@]}" "$app_path"
codesign --verify --deep --strict "$app_path"
printf '%s\n' "$app_path"
