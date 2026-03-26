#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
build_dir="${repo_root}/build/macos"
certificate_path="${build_dir}/taskers-codesign.p12"
keychain_path="${build_dir}/taskers-signing.keychain-db"
keychain_password="${TASKERS_MACOS_KEYCHAIN_PASSWORD:-taskers-temporary-keychain}"
identity="${TASKERS_MACOS_CODESIGN_IDENTITY:-}"

if [[ -z "${TASKERS_MACOS_CERTIFICATE_P12_BASE64:-}" ]]; then
  echo "TASKERS_MACOS_CERTIFICATE_P12_BASE64 is required" >&2
  exit 1
fi

if [[ -z "${TASKERS_MACOS_CERTIFICATE_PASSWORD:-}" ]]; then
  echo "TASKERS_MACOS_CERTIFICATE_PASSWORD is required" >&2
  exit 1
fi

if [[ -z "$identity" ]]; then
  echo "TASKERS_MACOS_CODESIGN_IDENTITY is required" >&2
  exit 1
fi

mkdir -p "$build_dir"
rm -f "$certificate_path" "$keychain_path"

python3 - "$certificate_path" <<'PY'
import base64
import os
import sys

decoded = base64.b64decode(os.environ["TASKERS_MACOS_CERTIFICATE_P12_BASE64"])
with open(sys.argv[1], "wb") as handle:
    handle.write(decoded)
PY

security create-keychain -p "$keychain_password" "$keychain_path"
security set-keychain-settings -lut 21600 "$keychain_path"
security unlock-keychain -p "$keychain_password" "$keychain_path"
security import "$certificate_path" \
  -k "$keychain_path" \
  -P "$TASKERS_MACOS_CERTIFICATE_PASSWORD" \
  -T /usr/bin/codesign \
  -T /usr/bin/security \
  -T /usr/bin/productbuild
security set-key-partition-list -S apple-tool:,apple: -k "$keychain_password" "$keychain_path"

existing_keychains=()
while IFS= read -r keychain; do
  keychain="${keychain//\"/}"
  keychain="${keychain#"${keychain%%[![:space:]]*}"}"
  keychain="${keychain%"${keychain##*[![:space:]]}"}"
  if [[ -n "$keychain" ]]; then
    existing_keychains+=("$keychain")
  fi
done < <(security list-keychains -d user)

security list-keychains -d user -s "$keychain_path" "${existing_keychains[@]}"
security default-keychain -d user -s "$keychain_path"

if ! security find-identity -v -p codesigning "$keychain_path" | grep -F "$identity" >/dev/null; then
  echo "codesigning identity not found in imported keychain: $identity" >&2
  exit 1
fi

printf '%s\n' "$keychain_path"
