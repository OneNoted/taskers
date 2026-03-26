#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build/macos"
GHOSTTY_DIR="${ROOT_DIR}/vendor/ghostty"
GHOSTTY_VERSION="$(sed -n 's/^.*\.version = "\([^"]*\)".*$/\1/p' "${GHOSTTY_DIR}/build.zig.zon" | head -n1)"
BUILD_MODE="${TASKERS_MACOS_DEP_MODE:-native}"
MODE_STAMP="${BUILD_DIR}/.deps-mode"

case "${BUILD_MODE}" in
  native|universal)
    ;;
  *)
    echo "unsupported TASKERS_MACOS_DEP_MODE=${BUILD_MODE}; expected native or universal" >&2
    exit 1
    ;;
esac

mkdir -p "${BUILD_DIR}/bin" "${BUILD_DIR}/resources"

pushd "${ROOT_DIR}" >/dev/null
if [[ "${BUILD_MODE}" == "universal" ]]; then
  cargo build --release -p taskers-macos-ffi --target aarch64-apple-darwin
  cargo build --release -p taskers-macos-ffi --target x86_64-apple-darwin
  cargo build --release -p taskers-cli --bin taskersctl --target aarch64-apple-darwin
  cargo build --release -p taskers-cli --bin taskersctl --target x86_64-apple-darwin
else
  cargo build --release -p taskers-macos-ffi
  cargo build --release -p taskers-cli --bin taskersctl
fi
popd >/dev/null

if [[ "${BUILD_MODE}" == "universal" ]]; then
  lipo -create \
    -output "${BUILD_DIR}/libtaskers_macos_ffi.a" \
    "${ROOT_DIR}/target/aarch64-apple-darwin/release/libtaskers_macos_ffi.a" \
    "${ROOT_DIR}/target/x86_64-apple-darwin/release/libtaskers_macos_ffi.a"
  lipo -create \
    -output "${BUILD_DIR}/bin/taskersctl" \
    "${ROOT_DIR}/target/aarch64-apple-darwin/release/taskersctl" \
    "${ROOT_DIR}/target/x86_64-apple-darwin/release/taskersctl"
else
  cp "${ROOT_DIR}/target/release/libtaskers_macos_ffi.a" "${BUILD_DIR}/libtaskers_macos_ffi.a"
  cp "${ROOT_DIR}/target/release/taskersctl" "${BUILD_DIR}/bin/taskersctl"
fi
chmod +x "${BUILD_DIR}/bin/taskersctl"

pushd "${GHOSTTY_DIR}" >/dev/null
# Ghostty defaults `emit_macos_app` to `emit_xcframework`, but Taskers only
# needs the embedded framework/resources for preview builds.
zig build \
  -Dapp-runtime=none \
  -Demit-xcframework=true \
  -Demit-macos-app=false \
  "-Dversion-string=${GHOSTTY_VERSION}" \
  "-Dxcframework-target=${BUILD_MODE}"
popd >/dev/null

rm -rf "${BUILD_DIR}/GhosttyKit.xcframework"
cp -R "${GHOSTTY_DIR}/macos/GhosttyKit.xcframework" "${BUILD_DIR}/GhosttyKit.xcframework"

rm -rf "${BUILD_DIR}/resources/ghostty" "${BUILD_DIR}/resources/terminfo"
cp -R "${GHOSTTY_DIR}/zig-out/share/ghostty" "${BUILD_DIR}/resources/ghostty"
cp -R "${GHOSTTY_DIR}/zig-out/share/terminfo" "${BUILD_DIR}/resources/terminfo"

printf '%s\n' "${BUILD_MODE}" > "${MODE_STAMP}"
