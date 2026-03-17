#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo_root/Cargo.toml" | head -n1)"
ghostty_version="$(sed -n 's/^.*\.version = "\([^"]*\)".*$/\1/p' "$repo_root/vendor/ghostty/build.zig.zon" | head -n1)"
target="${1:-$(rustc -vV | sed -n 's/^host: //p')}"
out_dir="${2:-$repo_root/dist}"
asset_name="taskers-linux-bundle-v${version}-${target}.tar.xz"
stage_dir="$(mktemp -d "${TMPDIR:-/tmp}/taskers-linux-bundle.XXXXXX")"
prefix_dir="$stage_dir/prefix"
bundle_dir="$stage_dir/bundle"

cleanup() {
  rm -rf "$stage_dir"
}
trap cleanup EXIT

(
  cd "$repo_root"
  cargo build --release -p taskers-gtk --bin taskers-gtk
  cargo build --release -p taskers-cli --bin taskersctl
)

(
  cd "$repo_root/vendor/ghostty"
  zig build taskers-bridge \
    -Dapp-runtime=gtk \
    -Demit-exe=false \
    -Dgtk-wayland=false \
    -Dstrip=true \
    -Di18n=false \
    "-Dversion-string=$ghostty_version" \
    --summary none \
    --prefix "$prefix_dir"
)

mkdir -p "$bundle_dir/bin" "$bundle_dir/ghostty/lib" "$bundle_dir/ghostty/shell-integration" "$bundle_dir/terminfo"
cp "$repo_root/target/release/taskers-gtk" "$bundle_dir/bin/taskers"
cp "$repo_root/target/release/taskersctl" "$bundle_dir/bin/taskersctl"
chmod +x "$bundle_dir/bin/taskers" "$bundle_dir/bin/taskersctl"
cp "$prefix_dir/lib/libtaskers_ghostty_bridge.so" "$bundle_dir/ghostty/lib/"
cp -R "$prefix_dir/share/ghostty/shell-integration/." "$bundle_dir/ghostty/shell-integration/"
cp -R "$prefix_dir/share/terminfo/." "$bundle_dir/terminfo/"
printf '%s\n' "$version" > "$bundle_dir/ghostty/.taskers-runtime-version"

mkdir -p "$out_dir"
XZ_OPT=-9 tar -C "$bundle_dir" -cJf "$out_dir/$asset_name" bin ghostty terminfo
printf '%s\n' "$out_dir/$asset_name"
