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
taskers_path="${app_bin_dir}/taskers"
app_binary_path="${app_bin_dir}/taskers-gtk"
taskersctl_path="${app_bin_dir}/taskersctl"

mkdir -p "${app_bin_dir}"

"${cargo_cmd}" install --path "${repo_root}/crates/taskers-app" --force --root "${cargo_root}"

if [[ ! -x "${taskers_path}" ]]; then
  echo "expected installed taskers binary at ${taskers_path}" >&2
  exit 1
fi

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

if [[ -d "${xdg_data_home}/applications" ]]; then
  if "${taskers_path}" --help >/dev/null 2>&1; then
    echo "refreshed desktop integration via ${taskers_path}"
  else
    echo "warning: failed to refresh desktop integration via ${taskers_path}" >&2
  fi
fi

echo "installed ${taskers_path}"
echo "installed ${app_binary_path}"
echo "installed ${taskersctl_path}"

resolved_taskers="$(command -v taskers 2>/dev/null || true)"
if [[ -n "${resolved_taskers}" ]]; then
  resolved_taskers="$(readlink -f "${resolved_taskers}" 2>/dev/null || printf '%s' "${resolved_taskers}")"
  if [[ "${resolved_taskers}" != "${taskers_path}" ]]; then
    echo "warning: plain 'taskers' currently resolves to ${resolved_taskers}, not ${taskers_path}" >&2
    echo "warning: for repo-local verification, launch ${app_binary_path} directly or prepend ${app_bin_dir} to PATH" >&2
  fi
fi

resolved_taskersctl="$(command -v taskersctl 2>/dev/null || true)"
if [[ -n "${resolved_taskersctl}" ]]; then
  resolved_taskersctl="$(readlink -f "${resolved_taskersctl}" 2>/dev/null || printf '%s' "${resolved_taskersctl}")"
  if [[ "${resolved_taskersctl}" != "${taskersctl_path}" ]]; then
    echo "warning: plain 'taskersctl' currently resolves to ${resolved_taskersctl}, not ${taskersctl_path}" >&2
  fi
fi

echo "for repo-local verification, prefer ${app_binary_path}"
