#!/usr/bin/env sh
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
approved_max_new_file_size=12586488

cd "$repo_root"

jj config set --repo snapshot.max-new-file-size "$approved_max_new_file_size"

printf '%s\n' \
  "Configured jj snapshot.max-new-file-size=$approved_max_new_file_size" \
  "Reason: vendor/ghostty contains approved upstream assets above JJ's default 1 MiB snapshot limit."
