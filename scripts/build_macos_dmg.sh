#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)"
derived_data_path="${1:-$repo_root/build/macos/DerivedData}"
app_path="${2:-$derived_data_path/Build/Products/Release/Taskers.app}"
out_dir="${3:-$repo_root/dist}"
asset_path="$out_dir/Taskers-v${version}-universal2.dmg"
staging_dir="$out_dir/.taskers-dmg-staging"

if [[ ! -d "$app_path" ]]; then
  echo "expected Taskers.app at $app_path" >&2
  exit 1
fi

mkdir -p "$out_dir"
rm -f "$asset_path"
rm -rf "$staging_dir"
mkdir -p "$staging_dir"
ditto "$app_path" "$staging_dir/Taskers.app"
ln -s /Applications "$staging_dir/Applications"
hdiutil create \
  -volname "Taskers" \
  -srcfolder "$staging_dir" \
  -ov \
  -format UDZO \
  "$asset_path"
rm -rf "$staging_dir"
printf '%s\n' "$asset_path"
