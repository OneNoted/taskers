#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${ROOT_DIR}"
BUILD_MODE="${TASKERS_MACOS_DEP_MODE:-native}"
MODE_STAMP="build/macos/.deps-mode"

if [[ ! -d build/macos/GhosttyKit.xcframework ]] \
  || [[ ! -f build/macos/libtaskers_macos_ffi.a ]] \
  || [[ ! -x build/macos/bin/taskersctl ]] \
  || [[ ! -f "${MODE_STAMP}" ]] \
  || [[ "$(<"${MODE_STAMP}")" != "${BUILD_MODE}" ]]; then
  bash scripts/macos-build-preview-deps.sh
fi

rm -rf macos/Taskers.xcodeproj
xcodegen generate --spec macos/project.yml --project macos
