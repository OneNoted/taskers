#!/usr/bin/env python3

from __future__ import annotations

import sys
from pathlib import Path

THRESHOLD_BYTES = 1_048_576
APPROVED_MAX_NEW_FILE_SIZE = 12_586_488
IGNORED_PATH_PREFIXES = (
    "vendor/ghostty/.flatpak-builder/",
    "vendor/ghostty/.nixos-test-history/",
    "vendor/ghostty/.zig-cache/",
    "vendor/ghostty/flatpak/builddir/",
    "vendor/ghostty/flatpak/repo/",
    "vendor/ghostty/result",
    "vendor/ghostty/test/ghostty",
    "vendor/ghostty/zig-cache/",
    "vendor/ghostty/zig-out/",
)
APPROVED_LARGE_FILES = {
    "vendor/ghostty/images/icons/icon_1024.png",
    "vendor/ghostty/images/icons/icon_1024@2x.png",
    "vendor/ghostty/macos/Assets.xcassets/Alternate Icons/RetroImage.imageset/macOS-AppIcon-1024px.png",
    "vendor/ghostty/pkg/simdutf/vendor/simdutf.cpp",
    "vendor/ghostty/pkg/wuffs/src/too_big.jpg",
    "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Bold.ttf",
    "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-BoldItalic.ttf",
    "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Italic.ttf",
    "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Regular.ttf",
    "vendor/ghostty/src/font/res/JuliaMono-Regular.ttf",
    "vendor/ghostty/src/font/res/NotoColorEmoji.ttf",
}


def iter_large_files(repo_root: Path) -> dict[str, int]:
    large_files: dict[str, int] = {}
    vendor_root = repo_root / "vendor" / "ghostty"

    for path in vendor_root.rglob("*"):
        if not path.is_file():
            continue

        size = path.stat().st_size
        if size <= THRESHOLD_BYTES:
            continue

        rel_path = path.relative_to(repo_root).as_posix()
        if rel_path.startswith(IGNORED_PATH_PREFIXES):
            continue

        large_files[rel_path] = size

    return large_files


def main() -> int:
    repo_root = Path(__file__).resolve().parent.parent
    actual_large_files = iter_large_files(repo_root)
    actual_paths = set(actual_large_files)

    unexpected = sorted(actual_paths - APPROVED_LARGE_FILES)
    missing = sorted(APPROVED_LARGE_FILES - actual_paths)
    over_limit = sorted(
        (path, size)
        for path, size in actual_large_files.items()
        if path in APPROVED_LARGE_FILES and size > APPROVED_MAX_NEW_FILE_SIZE
    )

    if unexpected or missing or over_limit:
        if unexpected:
            print("Unexpected Ghostty files above 1 MiB:", file=sys.stderr)
            for path in unexpected:
                print(f"  {path} ({actual_large_files[path]} bytes)", file=sys.stderr)

        if missing:
            print("Missing approved Ghostty large files:", file=sys.stderr)
            for path in missing:
                print(f"  {path}", file=sys.stderr)

        if over_limit:
            print(
                "Approved Ghostty large files exceed the repo-local jj snapshot limit:",
                file=sys.stderr,
            )
            for path, size in over_limit:
                print(f"  {path} ({size} bytes)", file=sys.stderr)
            print(
                "Update scripts/setup-jj.sh and review whether the larger assets should stay vendored.",
                file=sys.stderr,
            )

        return 1

    largest_path = max(actual_large_files, key=actual_large_files.get)
    largest_size = actual_large_files[largest_path]
    print(
        "Ghostty large-file check passed: "
        f"{len(actual_large_files)} approved files above 1 MiB, "
        f"largest is {largest_path} ({largest_size} bytes)."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
