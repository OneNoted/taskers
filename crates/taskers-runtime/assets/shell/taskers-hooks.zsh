[[ -n "${TASKERS_HOOKS_ZSH_LOADED:-}" ]] && return 0
export TASKERS_HOOKS_ZSH_LOADED=1

taskers__repo_root() {
  command -v git >/dev/null 2>&1 || return 0
  git -C "$PWD" rev-parse --show-toplevel 2>/dev/null || true
}

taskers__repo_branch() {
  command -v git >/dev/null 2>&1 || return 0
  git -C "$PWD" symbolic-ref --quiet --short HEAD 2>/dev/null \
    || git -C "$PWD" rev-parse --short HEAD 2>/dev/null \
    || true
}

taskers__classify_token() {
  case "$1" in
    codex) print -rn -- 'codex' ;;
    claude|claude-code) print -rn -- 'claude' ;;
    opencode) print -rn -- 'opencode' ;;
    aider) print -rn -- 'aider' ;;
    *) return 1 ;;
  esac
}

taskers__classify_command() {
  local -a words
  words=(${=1})

  while (( ${#words[@]} > 0 )); do
    case "${words[1]}" in
      *=*)
        words=("${words[@]:1}")
        ;;
      env)
        words=("${words[@]:1}")
        ;;
      npx|pnpx|bunx|uvx)
        (( ${#words[@]} > 1 )) || return 1
        taskers__classify_token "${words[2]}"
        return $?
        ;;
      pnpm|yarn)
        if (( ${#words[@]} > 2 )) && [[ "${words[2]}" = "dlx" ]]; then
          taskers__classify_token "${words[3]}"
          return $?
        fi
        return 1
        ;;
      *)
        taskers__classify_token "${words[1]}"
        return $?
        ;;
    esac
  done

  return 1
}

taskers__collect_metadata() {
  TASKERS_META_CWD=$PWD
  TASKERS_META_REPO_ROOT=$(taskers__repo_root)
  if [[ -n "$TASKERS_META_REPO_ROOT" ]]; then
    TASKERS_META_REPO_NAME=${TASKERS_META_REPO_ROOT:t}
    [[ -n "$TASKERS_META_REPO_NAME" ]] || TASKERS_META_REPO_NAME=/
    TASKERS_META_BRANCH=$(taskers__repo_branch)
  else
    TASKERS_META_REPO_NAME=
    TASKERS_META_BRANCH=
  fi

  TASKERS_META_AGENT=${TASKERS_ACTIVE_AGENT_KIND:-${TASKERS_PANE_AGENT_KIND:-shell}}
  TASKERS_META_LABEL=$TASKERS_META_REPO_NAME
  if [[ -z "$TASKERS_META_LABEL" ]]; then
    TASKERS_META_LABEL=${PWD:t}
    [[ -n "$TASKERS_META_LABEL" ]] || TASKERS_META_LABEL=/
  fi

  if [[ "$TASKERS_META_AGENT" = "shell" ]]; then
    TASKERS_META_TITLE=$TASKERS_META_LABEL
  else
    TASKERS_META_TITLE="${TASKERS_META_AGENT} :: ${TASKERS_META_LABEL}"
  fi
}

taskers__agent_active_for_kind() {
  case "$1" in
    started|progress|waiting_input)
      print -rn -- '1'
      ;;
    completed|error)
      print -rn -- '0'
      ;;
    *)
      if [[ -n "${TASKERS_ACTIVE_AGENT_KIND:-}" ]]; then
        print -rn -- '1'
      else
        print -rn -- '0'
      fi
      ;;
  esac
}

taskers__emit_with_metadata() {
  local kind=$1
  local message=${2:-}
  local agent_active
  local -a argv

  taskers__collect_metadata
  agent_active=$(taskers__agent_active_for_kind "$kind")

  [[ -x "${TASKERS_SHELL_BRIDGE_PATH:-}" ]] || return 0

  argv=(
    "$TASKERS_SHELL_BRIDGE_PATH"
    signal
    --kind "$kind"
    --title "$TASKERS_META_TITLE"
    --cwd "$TASKERS_META_CWD"
    --agent "$TASKERS_META_AGENT"
    --agent-active "$agent_active"
  )

  [[ -n "$TASKERS_META_REPO_NAME" ]] && argv+=(--repo "$TASKERS_META_REPO_NAME")
  [[ -n "$TASKERS_META_BRANCH" ]] && argv+=(--branch "$TASKERS_META_BRANCH")
  [[ -n "$message" ]] && argv+=(--message "$message")

  {
    exec </dev/null
    "${argv[@]}"
  } >/dev/null 2>&1 &!
}

taskers__emit_metadata_if_changed() {
  taskers__collect_metadata

  if [[ "${TASKERS_LAST_META_CWD:-}" = "$TASKERS_META_CWD" \
    && "${TASKERS_LAST_META_REPO_NAME:-}" = "$TASKERS_META_REPO_NAME" \
    && "${TASKERS_LAST_META_BRANCH:-}" = "$TASKERS_META_BRANCH" \
    && "${TASKERS_LAST_META_AGENT:-}" = "$TASKERS_META_AGENT" \
    && "${TASKERS_LAST_META_TITLE:-}" = "$TASKERS_META_TITLE" \
    && "${TASKERS_LAST_META_AGENT_ACTIVE:-}" = "$(taskers__agent_active_for_kind metadata)" ]]; then
    return 0
  fi

  export TASKERS_LAST_META_CWD=$TASKERS_META_CWD
  export TASKERS_LAST_META_REPO_NAME=$TASKERS_META_REPO_NAME
  export TASKERS_LAST_META_BRANCH=$TASKERS_META_BRANCH
  export TASKERS_LAST_META_AGENT=$TASKERS_META_AGENT
  export TASKERS_LAST_META_TITLE=$TASKERS_META_TITLE
  export TASKERS_LAST_META_AGENT_ACTIVE=$(taskers__agent_active_for_kind metadata)
  taskers__emit_with_metadata metadata
}

taskers__preexec() {
  local agent
  agent=$(taskers__classify_command "$1" || true)
  if [[ -n "$agent" ]]; then
    export TASKERS_PANE_AGENT_KIND=$agent
    export TASKERS_ACTIVE_AGENT_KIND=$agent
    taskers__emit_with_metadata started
  fi
}

taskers__precmd() {
  local exit_status=$?
  if [[ -n "${TASKERS_ACTIVE_AGENT_KIND:-}" ]]; then
    if (( exit_status == 0 )); then
      taskers__emit_with_metadata completed
    else
      taskers__emit_with_metadata error "Exited with status ${exit_status}"
    fi
    unset TASKERS_ACTIVE_AGENT_KIND
  fi

  taskers__emit_metadata_if_changed
}

taskers_signal() {
  [[ $# -gt 0 ]] || return 1
  local kind=$1
  shift
  taskers__emit_with_metadata "$kind" "$*"
}

taskers_waiting() {
  taskers_signal waiting_input "$@"
}

taskers_done() {
  taskers_signal completed "$@"
}

taskers_error() {
  taskers_signal error "$@"
}

taskers__normalize_backspace() {
  stty erase '^?' 2>/dev/null || true
  zmodload -F zsh/terminfo b:terminfo 2>/dev/null || true

  bindkey -M emacs '^?' backward-delete-char 2>/dev/null || true
  bindkey -M emacs '^H' backward-delete-char 2>/dev/null || true
  bindkey -M viins '^?' vi-backward-delete-char 2>/dev/null || true
  bindkey -M viins '^H' vi-backward-delete-char 2>/dev/null || true
  bindkey '^?' backward-delete-char 2>/dev/null || true
  bindkey '^H' backward-delete-char 2>/dev/null || true

  if [[ -n "${terminfo[kbs]:-}" ]]; then
    bindkey -M emacs "${terminfo[kbs]}" backward-delete-char 2>/dev/null || true
    bindkey -M viins "${terminfo[kbs]}" vi-backward-delete-char 2>/dev/null || true
    bindkey "${terminfo[kbs]}" backward-delete-char 2>/dev/null || true
  fi
}

typeset -ga preexec_functions
typeset -ga precmd_functions
preexec_functions+=(taskers__preexec)
precmd_functions+=(taskers__precmd)
taskers__normalize_backspace
taskers__emit_metadata_if_changed
