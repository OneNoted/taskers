#!/usr/bin/env bash
set -euo pipefail

if [[ -z "${TARGET_BUILD_DIR:-}" || -z "${UNLOCALIZED_RESOURCES_FOLDER_PATH:-}" ]]; then
  echo "xcode build paths are not available" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build/macos"
RESOURCES_DIR="${TARGET_BUILD_DIR}/${UNLOCALIZED_RESOURCES_FOLDER_PATH}"
HELPER_DIR="${RESOURCES_DIR}/bin"

mkdir -p "${HELPER_DIR}" "${RESOURCES_DIR}"

install -m 755 "${BUILD_DIR}/bin/taskersctl" "${HELPER_DIR}/taskersctl"

rm -rf "${RESOURCES_DIR}/ghostty" "${RESOURCES_DIR}/terminfo"
cp -R "${BUILD_DIR}/resources/ghostty" "${RESOURCES_DIR}/ghostty"
cp -R "${BUILD_DIR}/resources/terminfo" "${RESOURCES_DIR}/terminfo"
