#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)"
derived_data_path="${1:-$repo_root/build/macos/DerivedData}"
app_path="${2:-$derived_data_path/Build/Products/Release/Taskers.app}"
out_dir="${3:-$repo_root/dist}"
arch="${4:-$(uname -m)}"

case "$arch" in
  arm64|aarch64)
    target="aarch64-apple-darwin"
    ;;
  x86_64)
    target="x86_64-apple-darwin"
    ;;
  *)
    echo "unsupported macOS architecture: $arch" >&2
    exit 1
    ;;
esac

if [[ ! -d "$app_path" ]]; then
  echo "expected Taskers.app at $app_path" >&2
  exit 1
fi

mkdir -p "$out_dir"
ditto -c -k --sequesterRsrc --keepParent \
  "$app_path" \
  "$out_dir/taskers-macos-app-v${version}-${target}.zip"
printf '%s\n' "$out_dir/taskers-macos-app-v${version}-${target}.zip"
