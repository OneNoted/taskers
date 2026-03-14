#!/bin/sh
set -eu

proxy_name=$(basename -- "$0")
proxy_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

agent_kind=$proxy_name
case "$proxy_name" in
  claude-code) agent_kind=claude ;;
esac

agent_title=$agent_kind
case "$agent_kind" in
  codex) agent_title=Codex ;;
  claude) agent_title=Claude ;;
  opencode) agent_title=OpenCode ;;
  aider) agent_title=Aider ;;
esac

filtered_path=
old_ifs=${IFS:-" "}
IFS=:
for entry in ${PATH:-}; do
  if [ "$entry" = "$proxy_dir" ] || [ -z "$entry" ]; then
    continue
  fi
  if [ -n "$filtered_path" ]; then
    filtered_path="${filtered_path}:$entry"
  else
    filtered_path="$entry"
  fi
done
IFS=$old_ifs

if [ -z "$filtered_path" ]; then
  filtered_path=${PATH:-}
fi

real_binary=$(PATH="$filtered_path" command -v -- "$proxy_name" || true)
if [ -z "$real_binary" ]; then
  printf '%s: failed to locate real command in PATH\n' "$proxy_name" >&2
  exit 127
fi

emit_signal() {
  kind=$1
  message=${2-}
  repo_name=
  git_branch=
  if [ -n "${PWD:-}" ] && command -v git >/dev/null 2>&1; then
    repo_root=$(git -C "$PWD" rev-parse --show-toplevel 2>/dev/null || true)
    if [ -n "$repo_root" ]; then
      repo_name=$(basename -- "$repo_root")
      git_branch=$(git -C "$PWD" branch --show-current 2>/dev/null || true)
    fi
  fi
  [ -x "${TASKERS_CTL_PATH:-}" ] || return 0
  [ -n "${TASKERS_WORKSPACE_ID:-}" ] || return 0
  [ -n "${TASKERS_PANE_ID:-}" ] || return 0

  set -- signal --source shell --kind "$kind" --agent "$agent_kind" --title "$agent_title"
  if [ -n "${PWD:-}" ]; then
    set -- "$@" --cwd "$PWD"
  fi
  if [ -n "$repo_name" ]; then
    set -- "$@" --repo "$repo_name"
  fi
  if [ -n "$git_branch" ]; then
    set -- "$@" --branch "$git_branch"
  fi
  if [ -n "$message" ]; then
    set -- "$@" --message "$message"
  fi
  "$TASKERS_CTL_PATH" "$@" >/dev/null 2>&1 || true
}

if [ "${TASKERS_AGENT_PROXY_ACTIVE:-0}" != "1" ]; then
  emit_signal started
fi

set +e
TASKERS_AGENT_PROXY_ACTIVE=1 PATH="$filtered_path" "$real_binary" "$@"
status=$?
set -e

if [ "${TASKERS_AGENT_PROXY_ACTIVE:-0}" != "1" ]; then
  if [ "$status" -eq 0 ]; then
    emit_signal completed
  else
    emit_signal error
  fi
fi

exit "$status"
