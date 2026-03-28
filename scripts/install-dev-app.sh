#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
xdg_data_home="${XDG_DATA_HOME:-$HOME/.local/share}"

if [[ -n "${CARGO:-}" ]]; then
  cargo_cmd="${CARGO}"
elif [[ -n "${CARGO_HOME:-}" ]]; then
  cargo_cmd="${CARGO_HOME}/bin/cargo"
else
  cargo_cmd="${HOME}/.cargo/bin/cargo"
fi

if [[ ! -x "${cargo_cmd}" ]]; then
  cargo_cmd="$(command -v cargo)"
fi

if [[ -z "${cargo_cmd:-}" || ! -x "${cargo_cmd}" ]]; then
  echo "cargo executable not found" >&2
  exit 1
fi

cargo_root="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}"
app_bin_dir="${cargo_root}/bin"
app_binary_path="${app_bin_dir}/taskers-gtk"
taskersctl_path="${app_bin_dir}/taskersctl"
release_install_root="${TASKERS_INSTALL_ROOT:-${xdg_data_home}/taskers/releases}"

workspace_version="$(
  REPO_ROOT="${repo_root}" python3 - <<'PY'
import os
import pathlib
import tomllib

repo_root = pathlib.Path(os.environ["REPO_ROOT"])
with open(repo_root / "Cargo.toml", "rb") as handle:
    cargo = tomllib.load(handle)

print(cargo["workspace"]["package"]["version"])
PY
)"
target_triple="$(rustc -vV | sed -n 's/^host: //p')"
managed_bundle_dir="${release_install_root}/${workspace_version}/${target_triple}"

mkdir -p "${app_bin_dir}"

"${cargo_cmd}" install --path "${repo_root}/crates/taskers-app" --force --root "${cargo_root}"

if [[ ! -x "${app_binary_path}" ]]; then
  echo "expected installed app at ${app_binary_path}" >&2
  exit 1
fi

if [[ ! -x "${taskersctl_path}" ]]; then
  echo "expected installed control binary at ${taskersctl_path}" >&2
  exit 1
fi

legacy_wrapper_path="${app_bin_dir}/taskers-gtk-desktop-launch"
if [[ -e "${legacy_wrapper_path}" || -L "${legacy_wrapper_path}" ]]; then
  rm -f "${legacy_wrapper_path}"
  echo "removed ${legacy_wrapper_path}"
fi

bundle_synced=false
if launcher_path="$(command -v taskers 2>/dev/null)"; then
  if "${launcher_path}" --help >/dev/null 2>&1; then
    echo "refreshed desktop integration via ${launcher_path}"
  else
    echo "warning: failed to refresh desktop integration via ${launcher_path}" >&2
  fi

  if [[ -x "${managed_bundle_dir}/bin/taskers" && -x "${managed_bundle_dir}/bin/taskersctl" ]]; then
    install -m 755 "${app_binary_path}" "${managed_bundle_dir}/bin/taskers"
    install -m 755 "${taskersctl_path}" "${managed_bundle_dir}/bin/taskersctl"
    bundle_synced=true
    echo "synced launcher-managed bundle at ${managed_bundle_dir}"
  else
    echo "warning: launcher-managed bundle not found at ${managed_bundle_dir}" >&2
  fi
elif [[ -d "${xdg_data_home}/applications" ]]; then
  echo "warning: desktop integration not refreshed because no installed taskers launcher was found in PATH" >&2
fi

echo "installed ${app_binary_path}"
echo "installed ${taskersctl_path}"
if [[ "${bundle_synced}" == true ]]; then
  echo "desktop entry and terminal launcher now use the synced local binaries"
else
  echo "desktop entry is managed by the installed taskers launcher"
fi
