#!/usr/bin/env bash
set -euo pipefail

THRESHOLD_BYTES=1048576
APPROVED_MAX_NEW_FILE_SIZE=12586488

IGNORED_PATH_PREFIXES=(
  "vendor/ghostty/.flatpak-builder/"
  "vendor/ghostty/.nixos-test-history/"
  "vendor/ghostty/.zig-cache/"
  "vendor/ghostty/flatpak/builddir/"
  "vendor/ghostty/flatpak/repo/"
  "vendor/ghostty/result"
  "vendor/ghostty/test/ghostty"
  "vendor/ghostty/zig-cache/"
  "vendor/ghostty/zig-out/"
)

APPROVED_LARGE_FILES=(
  "vendor/ghostty/images/icons/icon_1024.png"
  "vendor/ghostty/images/icons/icon_1024@2x.png"
  "vendor/ghostty/macos/Assets.xcassets/Alternate Icons/RetroImage.imageset/macOS-AppIcon-1024px.png"
  "vendor/ghostty/pkg/simdutf/vendor/simdutf.cpp"
  "vendor/ghostty/pkg/wuffs/src/too_big.jpg"
  "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Bold.ttf"
  "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-BoldItalic.ttf"
  "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Italic.ttf"
  "vendor/ghostty/src/font/res/JetBrainsMonoNerdFont-Regular.ttf"
  "vendor/ghostty/src/font/res/JuliaMono-Regular.ttf"
  "vendor/ghostty/src/font/res/NotoColorEmoji.ttf"
)

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
VENDOR_ROOT="$REPO_ROOT/vendor/ghostty"

declare -A approved_set=()
declare -A actual_sizes=()

for path in "${APPROVED_LARGE_FILES[@]}"; do
  approved_set["$path"]=1
done

while IFS= read -r -d '' path; do
  rel_path=${path#"$REPO_ROOT"/}
  skip=0
  for prefix in "${IGNORED_PATH_PREFIXES[@]}"; do
    if [[ "$rel_path" == "$prefix"* ]]; then
      skip=1
      break
    fi
  done
  if (( skip )); then
    continue
  fi

  size=$(stat -c '%s' "$path")
  actual_sizes["$rel_path"]=$size
done < <(find "$VENDOR_ROOT" -type f -size +"${THRESHOLD_BYTES}"c -print0)

unexpected=()
for path in "${!actual_sizes[@]}"; do
  if [[ -z "${approved_set[$path]:-}" ]]; then
    unexpected+=("$path")
  fi
done

missing=()
for path in "${APPROVED_LARGE_FILES[@]}"; do
  if [[ -z "${actual_sizes[$path]:-}" ]]; then
    missing+=("$path")
  fi
done

over_limit=()
for path in "${APPROVED_LARGE_FILES[@]}"; do
  size=${actual_sizes[$path]:-}
  if [[ -n "$size" ]] && (( size > APPROVED_MAX_NEW_FILE_SIZE )); then
    over_limit+=("$path|$size")
  fi
done

if (( ${#unexpected[@]} > 0 || ${#missing[@]} > 0 || ${#over_limit[@]} > 0 )); then
  if (( ${#unexpected[@]} > 0 )); then
    printf 'Unexpected Ghostty files above 1 MiB:\n' >&2
    while IFS= read -r path; do
      printf '  %s (%s bytes)\n' "$path" "${actual_sizes[$path]}" >&2
    done < <(printf '%s\n' "${unexpected[@]}" | sort)
  fi

  if (( ${#missing[@]} > 0 )); then
    printf 'Missing approved Ghostty large files:\n' >&2
    while IFS= read -r path; do
      printf '  %s\n' "$path" >&2
    done < <(printf '%s\n' "${missing[@]}" | sort)
  fi

  if (( ${#over_limit[@]} > 0 )); then
    printf 'Approved Ghostty large files exceed the repo-local jj snapshot limit:\n' >&2
    while IFS='|' read -r path size; do
      printf '  %s (%s bytes)\n' "$path" "$size" >&2
    done < <(printf '%s\n' "${over_limit[@]}" | sort)
    printf '%s\n' 'Update scripts/setup-jj.sh and review whether the larger assets should stay vendored.' >&2
  fi

  exit 1
fi

largest_path=
largest_size=0
for path in "${!actual_sizes[@]}"; do
  size=${actual_sizes[$path]}
  if (( size > largest_size )); then
    largest_path=$path
    largest_size=$size
  fi
done

printf 'Ghostty large-file check passed: %s approved files above 1 MiB, largest is %s (%s bytes).\n' \
  "${#actual_sizes[@]}" "$largest_path" "$largest_size"
