#!/usr/bin/env bash
set -euo pipefail

version="0.16.0"
sha256="01feb8263fe7a450b0a9fed0fd54cf88947aaf00f86cc7da345f8b39a0e7bd30"
archive_url="https://gitlab.gnome.org/GNOME/blueprint-compiler/-/archive/v${version}/blueprint-compiler-v${version}.tar.gz"
prefix="${1:-${RUNNER_TEMP:-/tmp}/taskers-blueprint-compiler}"

require_command() {
  local command_name="$1"
  if ! command -v "$command_name" >/dev/null 2>&1; then
    printf 'missing required command: %s\n' "$command_name" >&2
    exit 1
  fi
}

version_at_least() {
  python3 - "$1" "$2" <<'PY'
import sys

def normalize(value: str) -> tuple[int, ...]:
    return tuple(int(part) for part in value.strip().split("."))

current = normalize(sys.argv[1])
minimum = normalize(sys.argv[2])
sys.exit(0 if current >= minimum else 1)
PY
}

installed_version() {
  local binary_path="$1"
  "$binary_path" --version | awk '{print $NF}'
}

require_command python3
require_command tar
require_command meson
require_command ninja

existing_binary="$prefix/bin/blueprint-compiler"
if [[ -x "$existing_binary" ]]; then
  current_version="$(installed_version "$existing_binary")"
  if version_at_least "$current_version" "$version"; then
    printf 'blueprint-compiler %s already installed at %s\n' "$current_version" "$prefix"
    exit 0
  fi
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/taskers-blueprint.XXXXXX")"
archive_path="$work_dir/blueprint-compiler-v${version}.tar.gz"
source_dir="$work_dir/blueprint-compiler-v${version}"
build_dir="$work_dir/build"

cleanup() {
  rm -rf "$work_dir"
}
trap cleanup EXIT

python3 - "$archive_url" "$archive_path" "$sha256" <<'PY'
import hashlib
import pathlib
import sys
import urllib.request

url, dest, expected = sys.argv[1:4]
data = urllib.request.urlopen(url, timeout=60).read()
digest = hashlib.sha256(data).hexdigest()
if digest != expected:
    raise SystemExit(f"blueprint-compiler sha256 mismatch: expected {expected}, got {digest}")
pathlib.Path(dest).write_bytes(data)
PY

tar -xf "$archive_path" -C "$work_dir"
rm -rf "$prefix"
meson setup "$build_dir" "$source_dir" --prefix "$prefix"
meson install -C "$build_dir"

current_version="$(installed_version "$prefix/bin/blueprint-compiler")"
if ! version_at_least "$current_version" "$version"; then
  printf 'expected blueprint-compiler >= %s, got %s\n' "$version" "$current_version" >&2
  exit 1
fi

printf '%s\n' "$prefix"
