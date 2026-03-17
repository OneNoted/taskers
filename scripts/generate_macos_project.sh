#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"

if [[ ! -d build/macos/GhosttyKit.xcframework ]] \
  || [[ ! -f build/macos/libtaskers_macos_ffi.a ]] \
  || [[ ! -x build/macos/bin/taskersctl ]]; then
  bash scripts/macos-build-preview-deps.sh
fi

rm -rf macos/Taskers.xcodeproj
xcodegen generate --spec macos/project.yml --project macos
