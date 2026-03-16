#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BUILD_DIR="${ROOT_DIR}/build/macos"
GHOSTTY_DIR="${ROOT_DIR}/vendor/ghostty"

mkdir -p "${BUILD_DIR}/bin" "${BUILD_DIR}/resources"

pushd "${ROOT_DIR}" >/dev/null
cargo build --release -p taskers-macos-ffi
cargo build --release -p taskers-cli --bin taskersctl
popd >/dev/null

cp "${ROOT_DIR}/target/release/libtaskers_macos_ffi.a" "${BUILD_DIR}/libtaskers_macos_ffi.a"
cp "${ROOT_DIR}/target/release/taskersctl" "${BUILD_DIR}/bin/taskersctl"
chmod +x "${BUILD_DIR}/bin/taskersctl"

pushd "${GHOSTTY_DIR}" >/dev/null
zig build -Dapp-runtime=none -Demit-xcframework=true -Dxcframework-target=native
popd >/dev/null

rm -rf "${BUILD_DIR}/GhosttyKit.xcframework"
cp -R "${GHOSTTY_DIR}/zig-out/macos/GhosttyKit.xcframework" "${BUILD_DIR}/GhosttyKit.xcframework"

rm -rf "${BUILD_DIR}/resources/ghostty" "${BUILD_DIR}/resources/terminfo"
cp -R "${GHOSTTY_DIR}/zig-out/share/ghostty" "${BUILD_DIR}/resources/ghostty"
cp -R "${GHOSTTY_DIR}/zig-out/share/terminfo" "${BUILD_DIR}/resources/terminfo"
