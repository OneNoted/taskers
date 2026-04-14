from __future__ import annotations

import json
import hashlib
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "render_taskers_bin_aur.py"
WRAPPER_SOURCE = REPO_ROOT / "packaging" / "aur" / "taskers-bin" / "taskers-wrapper.sh"
DESKTOP_TEMPLATE = REPO_ROOT / "crates" / "taskers-app" / "assets" / "taskers.desktop.in"
ICON_SOURCE = REPO_ROOT / "crates" / "taskers-app" / "assets" / "taskers.svg"
LICENSE_SOURCE = REPO_ROOT / "LICENSE"


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class RenderTaskersBinAurTests(unittest.TestCase):
    def test_renders_expected_sources_from_manifest(self) -> None:
        manifest = {
            "version": "1.2.3",
            "artifacts": {
                "x86_64-unknown-linux-gnu": {
                    "kind": "linux_bundle_v1",
                    "url": "https://example.invalid/taskers-linux-bundle-v1.2.3.tar.xz",
                    "sha256": "ABCDEF1234567890",
                }
            },
        }

        with tempfile.TemporaryDirectory() as tmpdir:
            temp_root = Path(tmpdir)
            manifest_path = temp_root / "manifest.json"
            output_dir = temp_root / "out"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT_PATH),
                    "--version",
                    "1.2.3",
                    "--pkgrel",
                    "7",
                    "--repository",
                    "ExampleOrg/taskers",
                    "--manifest-url",
                    manifest_path.as_uri(),
                    "--output-dir",
                    str(output_dir),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            payload = json.loads(result.stdout)
            self.assertEqual(payload["version"], "1.2.3")
            self.assertEqual(payload["pkgrel"], "7")
            self.assertEqual(payload["manifest_url"], manifest_path.as_uri())
            self.assertEqual(
                payload["bundle_url"],
                "https://example.invalid/taskers-linux-bundle-v1.2.3.tar.xz",
            )
            self.assertEqual(payload["bundle_sha256"], "abcdef1234567890")

            wrapper_path = output_dir / "taskers-entrypoint.sh"
            self.assertTrue(wrapper_path.exists())
            self.assertEqual(wrapper_path.read_bytes(), WRAPPER_SOURCE.read_bytes())
            self.assertEqual(stat.S_IMODE(wrapper_path.stat().st_mode), 0o755)

            desktop_path = output_dir / "dev.taskers.app.desktop"
            expected_desktop = DESKTOP_TEMPLATE.read_text(encoding="utf-8").replace(
                "{{EXEC}}", "taskers"
            )
            self.assertEqual(desktop_path.read_text(encoding="utf-8"), expected_desktop)
            expected_desktop_sha = sha256_bytes(expected_desktop.encode("utf-8"))
            expected_wrapper_sha = sha256_bytes(WRAPPER_SOURCE.read_bytes())
            expected_icon_sha = sha256_bytes(ICON_SOURCE.read_bytes())
            expected_license_sha = sha256_bytes(LICENSE_SOURCE.read_bytes())

            pkgbuild = (output_dir / "PKGBUILD").read_text(encoding="utf-8")
            self.assertIn("pkgver=1.2.3", pkgbuild)
            self.assertIn("pkgrel=7", pkgbuild)
            self.assertIn(
                "https://example.invalid/taskers-linux-bundle-v1.2.3.tar.xz",
                pkgbuild,
            )
            self.assertIn("'abcdef1234567890'", pkgbuild)
            self.assertIn(f"'{expected_wrapper_sha}'", pkgbuild)
            self.assertIn(f"'{expected_desktop_sha}'", pkgbuild)
            self.assertIn(f"'{expected_icon_sha}'", pkgbuild)
            self.assertIn(f"'{expected_license_sha}'", pkgbuild)

            srcinfo = (output_dir / ".SRCINFO").read_text(encoding="utf-8")
            self.assertIn("pkgver = 1.2.3", srcinfo)
            self.assertIn("pkgrel = 7", srcinfo)
            self.assertIn("sha256sums = abcdef1234567890", srcinfo)
            self.assertIn(f"sha256sums = {expected_wrapper_sha}", srcinfo)
            self.assertIn(f"sha256sums = {expected_desktop_sha}", srcinfo)
            self.assertIn(f"sha256sums = {expected_icon_sha}", srcinfo)
            self.assertIn(f"sha256sums = {expected_license_sha}", srcinfo)

    def test_fails_when_manifest_version_does_not_match(self) -> None:
        manifest = {
            "version": "9.9.9",
            "artifacts": {
                "x86_64-unknown-linux-gnu": {
                    "kind": "linux_bundle_v1",
                    "url": "https://example.invalid/taskers-linux-bundle-v9.9.9.tar.xz",
                    "sha256": "deadbeef",
                }
            },
        }

        with tempfile.TemporaryDirectory() as tmpdir:
            temp_root = Path(tmpdir)
            manifest_path = temp_root / "manifest.json"
            manifest_path.write_text(json.dumps(manifest), encoding="utf-8")

            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPT_PATH),
                    "--version",
                    "1.2.3",
                    "--manifest-url",
                    manifest_path.as_uri(),
                    "--output-dir",
                    str(temp_root / "out"),
                ],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
                check=False,
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertIn("does not match requested '1.2.3'", result.stderr)


if __name__ == "__main__":
    unittest.main()
