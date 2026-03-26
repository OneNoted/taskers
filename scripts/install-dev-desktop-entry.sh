#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
xdg_data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
launcher_home="${HOME}/.local/bin"
desktop_entry_path="${xdg_data_home}/applications/dev.taskers.app.desktop"
launcher_path="${launcher_home}/taskers-dev"

if [[ -n "${CARGO:-}" ]]; then
  cargo_bin="${CARGO}"
elif [[ -n "${CARGO_HOME:-}" ]]; then
  cargo_bin="${CARGO_HOME}/bin/cargo"
else
  cargo_bin="${HOME}/.cargo/bin/cargo"
fi

if [[ ! -x "${cargo_bin}" ]]; then
  cargo_bin="$(command -v cargo)"
fi

mkdir -p "${launcher_home}" "$(dirname -- "${desktop_entry_path}")"

cat > "${launcher_path}" <<EOF
#!/usr/bin/env sh
set -eu

exec "${cargo_bin}" run --manifest-path "${repo_root}/Cargo.toml" -p taskers-gtk --bin taskers-gtk -- "\$@"
EOF
chmod +x "${launcher_path}"

cat > "${desktop_entry_path}" <<EOF
[Desktop Entry]
Version=1.0
Type=Application
Name=Taskers
Comment=Agent-first terminal workspace
Exec=${launcher_path}
TryExec=${launcher_path}
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

echo "installed ${desktop_entry_path}"
