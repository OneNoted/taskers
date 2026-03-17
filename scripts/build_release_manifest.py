#!/usr/bin/env python3
import argparse
import hashlib
import json
from pathlib import Path


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_entry(path: Path, kind: str, minimum_os_version: str | None = None) -> dict:
    entry = {
        "kind": kind,
        "url": f"{base_url}/{path.name}",
        "sha256": sha256(path),
        "size_bytes": path.stat().st_size,
    }
    if minimum_os_version is not None:
        entry["minimum_os_version"] = minimum_os_version
    return entry


parser = argparse.ArgumentParser()
parser.add_argument("--repo-root", default=Path(__file__).resolve().parent.parent, type=Path)
parser.add_argument("--dist-dir", type=Path)
parser.add_argument("--version")
parser.add_argument("--base-url")
parser.add_argument("--output", type=Path)
args = parser.parse_args()

repo_root = args.repo_root.resolve()
version = args.version
if version is None:
    for line in (repo_root / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        if line.startswith("version = "):
            version = line.split('"')[1]
            break
if version is None:
    raise SystemExit("failed to discover version from Cargo.toml")

dist_dir = (args.dist_dir or repo_root / "dist").resolve()
base_url = args.base_url or f"https://github.com/OneNoted/taskers/releases/download/v{version}"
output_path = args.output or dist_dir / f"taskers-manifest-v{version}.json"

artifacts = {}
linux_bundle = dist_dir / f"taskers-linux-bundle-v{version}-x86_64-unknown-linux-gnu.tar.xz"
if linux_bundle.exists():
    artifacts["x86_64-unknown-linux-gnu"] = artifact_entry(linux_bundle, "linux_bundle_v1")

for target in ("aarch64-apple-darwin", "x86_64-apple-darwin"):
    archive = dist_dir / f"taskers-macos-app-v{version}-{target}.zip"
    if archive.exists():
        artifacts[target] = artifact_entry(
            archive,
            "macos_app_zip_v1",
            minimum_os_version="14.0",
        )

manual_downloads = {}
universal_dmg = dist_dir / f"Taskers-v{version}-universal2.dmg"
if universal_dmg.exists():
    manual_downloads["macos_universal2_dmg"] = {
        "url": f"{base_url}/{universal_dmg.name}",
        "sha256": sha256(universal_dmg),
    }

manifest = {
    "version": version,
    "artifacts": artifacts,
    "manual_downloads": manual_downloads,
}

output_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(output_path)
