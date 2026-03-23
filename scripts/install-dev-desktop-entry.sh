#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
xdg_data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop_entry_path="${xdg_data_home}/applications/dev.taskers.app.desktop"

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

mkdir -p "${app_bin_dir}" "$(dirname -- "${desktop_entry_path}")"

"${cargo_cmd}" install --path "${repo_root}/crates/taskers-app" --force --root "${cargo_root}"

if [[ ! -x "${app_binary_path}" ]]; then
  echo "expected installed app at ${app_binary_path}" >&2
  exit 1
fi

cat > "${desktop_entry_path}" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Taskers
Comment=Agent-first terminal workspace
Exec=${app_binary_path}
TryExec=${app_binary_path}
Icon=taskers
Terminal=false
Categories=Development;
StartupNotify=true
StartupWMClass=taskers
X-GNOME-UsesNotifications=true
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "${xdg_data_home}/applications"
fi

echo "installed ${app_binary_path}"
echo "installed ${desktop_entry_path}"
