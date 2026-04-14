#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path
from textwrap import dedent
from urllib.parse import urlparse
from urllib.request import urlopen

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_REPOSITORY = "OneNoted/taskers"
TARGET_TRIPLE = "x86_64-unknown-linux-gnu"
WRAPPER_NAME = "taskers-entrypoint.sh"
DESKTOP_NAME = "dev.taskers.app.desktop"
ICON_NAME = "taskers.svg"
LICENSE_NAME = "LICENSE"
WRAPPER_SOURCE = REPO_ROOT / "packaging/aur/taskers-bin/taskers-wrapper.sh"

PKGBUILD_TEMPLATE = """# Maintained automatically from {repository}\npkgname=taskers-bin\npkgver={version}\npkgrel={pkgrel}\npkgdesc='Agent-first terminal workspace (published Linux bundle)'\narch=('x86_64')\nurl='https://github.com/{repository}'\nlicense=('MIT')\ndepends=('glibc' 'gtk4' 'libadwaita' 'webkitgtk-6.0')\noptdepends=(\n  'niri: focus an existing Taskers window from desktop launches'\n  'xdg-desktop-portal-gtk: improve desktop portal support'\n)\nconflicts=('taskers' 'taskers-git')\nprovides=('taskers')\nsource=(\n  'taskers-linux-bundle-v{version}-{target_triple}.tar.xz::{bundle_url}'\n  '{wrapper_name}'\n  '{desktop_name}'\n  '{icon_name}'\n  '{license_name}'\n)\nsha256sums=(\n  '{bundle_sha256}'\n  '{wrapper_sha256}'\n  '{desktop_sha256}'\n  '{icon_sha256}'\n  '{license_sha256}'\n)\n\npackage() {{\n  install -dm755 \"$pkgdir/opt/taskers\" \"$pkgdir/usr/bin\" \\\n    \"$pkgdir/usr/share/applications\" \\\n    \"$pkgdir/usr/share/icons/hicolor/scalable/apps\" \\\n    \"$pkgdir/usr/share/licenses/${{pkgname}}\"\n\n  cp -a \"$srcdir/bin\" \"$pkgdir/opt/taskers/\"\n  cp -a \"$srcdir/ghostty\" \"$pkgdir/opt/taskers/\"\n  cp -a \"$srcdir/terminfo\" \"$pkgdir/opt/taskers/\"\n\n  for bin in taskers taskersctl taskers-terminald; do\n    install -m755 \"$srcdir/{wrapper_name}\" \"$pkgdir/usr/bin/$bin\"\n  done\n\n  install -m644 \"$srcdir/{desktop_name}\" \\\n    \"$pkgdir/usr/share/applications/dev.taskers.app.desktop\"\n  install -m644 \"$srcdir/{icon_name}\" \\\n    \"$pkgdir/usr/share/icons/hicolor/scalable/apps/taskers.svg\"\n  install -m644 \"$srcdir/{license_name}\" \\\n    \"$pkgdir/usr/share/licenses/${{pkgname}}/LICENSE\"\n}}\n"""


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def read_source_bytes(source: str) -> bytes:
    parsed = urlparse(source)
    if parsed.scheme in {"http", "https", "file"}:
        with urlopen(source) as handle:
            return handle.read()
    return Path(source).read_bytes()


def resolve_manifest_url(version: str, repository: str, manifest_url: str | None) -> str:
    if manifest_url:
        return manifest_url
    return f"https://github.com/{repository}/releases/download/v{version}/taskers-manifest-v{version}.json"


def load_bundle_artifact(version: str, repository: str, manifest_url: str | None) -> dict:
    manifest_source = resolve_manifest_url(version, repository, manifest_url)
    manifest = json.loads(read_source_bytes(manifest_source))
    if manifest.get("version") != version:
        raise SystemExit(
            f"manifest version {manifest.get('version')!r} does not match requested {version!r}"
        )
    artifacts = manifest.get("artifacts", {})
    artifact = artifacts.get(TARGET_TRIPLE)
    if not artifact:
        raise SystemExit(f"manifest does not contain an artifact for {TARGET_TRIPLE}")
    if artifact.get("kind") != "linux_bundle_v1":
        raise SystemExit(f"unexpected artifact kind: {artifact.get('kind')!r}")
    return artifact


def render_desktop_entry() -> bytes:
    template = (REPO_ROOT / "crates/taskers-app/assets/taskers.desktop.in").read_text(encoding="utf-8")
    return template.replace("{{EXEC}}", "taskers").encode("utf-8")


def write_asset(path: Path, content: bytes, executable: bool = False) -> None:
    path.write_bytes(content)
    if executable:
        path.chmod(0o755)


def write_text(path: Path, content: str, executable: bool = False) -> None:
    path.write_text(content, encoding="utf-8")
    if executable:
        path.chmod(0o755)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--pkgrel", default="1")
    parser.add_argument("--repository", default=DEFAULT_REPOSITORY)
    parser.add_argument("--manifest-url")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()

    artifact = load_bundle_artifact(args.version, args.repository, args.manifest_url)
    bundle_url = artifact["url"]
    bundle_sha256 = artifact["sha256"].lower()

    output_dir = args.output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)

    wrapper_path = output_dir / WRAPPER_NAME
    wrapper_bytes = WRAPPER_SOURCE.read_bytes()
    write_asset(wrapper_path, wrapper_bytes, executable=True)

    desktop_bytes = render_desktop_entry()
    desktop_path = output_dir / DESKTOP_NAME
    write_asset(desktop_path, desktop_bytes)

    icon_path = output_dir / ICON_NAME
    icon_bytes = (REPO_ROOT / "crates/taskers-app/assets/taskers.svg").read_bytes()
    write_asset(icon_path, icon_bytes)

    license_path = output_dir / LICENSE_NAME
    license_bytes = (REPO_ROOT / "LICENSE").read_bytes()
    write_asset(license_path, license_bytes)

    pkgbuild = PKGBUILD_TEMPLATE.format(
        repository=args.repository,
        version=args.version,
        pkgrel=args.pkgrel,
        target_triple=TARGET_TRIPLE,
        bundle_url=bundle_url,
        bundle_sha256=bundle_sha256,
        wrapper_name=WRAPPER_NAME,
        wrapper_sha256=sha256_bytes(wrapper_bytes),
        desktop_name=DESKTOP_NAME,
        desktop_sha256=sha256_bytes(desktop_bytes),
        icon_name=ICON_NAME,
        icon_sha256=sha256_bytes(icon_bytes),
        license_name=LICENSE_NAME,
        license_sha256=sha256_bytes(license_bytes),
    )
    pkgbuild_path = output_dir / "PKGBUILD"
    write_text(pkgbuild_path, pkgbuild)

    srcinfo = dedent(
        f"""\
        pkgbase = taskers-bin
        	pkgdesc = Agent-first terminal workspace (published Linux bundle)
        	pkgver = {args.version}
        	pkgrel = {args.pkgrel}
        	url = https://github.com/{args.repository}
        	arch = x86_64
        	license = MIT
        	depends = glibc
        	depends = gtk4
        	depends = libadwaita
        	depends = webkitgtk-6.0
        	optdepends = niri: focus an existing Taskers window from desktop launches
        	optdepends = xdg-desktop-portal-gtk: improve desktop portal support
        	provides = taskers
        	conflicts = taskers
        	conflicts = taskers-git
        	source = taskers-linux-bundle-v{args.version}-{TARGET_TRIPLE}.tar.xz::{bundle_url}
        	source = {WRAPPER_NAME}
        	source = {DESKTOP_NAME}
        	source = {ICON_NAME}
        	source = {LICENSE_NAME}
        	sha256sums = {bundle_sha256}
        	sha256sums = {sha256_bytes(wrapper_bytes)}
        	sha256sums = {sha256_bytes(desktop_bytes)}
        	sha256sums = {sha256_bytes(icon_bytes)}
        	sha256sums = {sha256_bytes(license_bytes)}

        pkgname = taskers-bin
        """
    )
    write_text(output_dir / ".SRCINFO", srcinfo)

    print(json.dumps({
        "version": args.version,
        "pkgrel": args.pkgrel,
        "manifest_url": resolve_manifest_url(args.version, args.repository, args.manifest_url),
        "bundle_url": bundle_url,
        "bundle_sha256": bundle_sha256,
        "output_dir": str(output_dir),
    }, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
