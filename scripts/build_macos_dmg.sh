#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)"
derived_data_path="${1:-$repo_root/build/macos/DerivedData}"
app_path="${2:-$derived_data_path/Build/Products/Release/Taskers.app}"
out_dir="${3:-$repo_root/dist}"
asset_path="$out_dir/Taskers-v${version}-universal2.dmg"

if [[ ! -d "$app_path" ]]; then
  echo "expected Taskers.app at $app_path" >&2
  exit 1
fi

mkdir -p "$out_dir"
rm -f "$asset_path"
hdiutil create \
  -volname "Taskers" \
  -srcfolder "$app_path" \
  -ov \
  -format UDZO \
  "$asset_path"
printf '%s\n' "$asset_path"
