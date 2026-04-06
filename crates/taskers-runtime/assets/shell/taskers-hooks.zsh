[[ -n "${TASKERS_HOOKS_ZSH_LOADED:-}" ]] && return 0
export TASKERS_HOOKS_ZSH_LOADED=1

typeset -g TASKERS_OSC133_EXECUTING=0
typeset -g TASKERS_OSC133_MARK_A=$'%{\e]133;A;redraw=1;cl=line\a%}'
typeset -g TASKERS_OSC133_MARK_A_SECONDARY=$'%{\e]133;A;k=s\a%}'
typeset -g TASKERS_OSC133_MARK_B=$'%{\e]133;B\a%}'

taskers__osc133_enabled() {
  taskers__context_tty_matches
}

taskers__osc133_mark_prompt() {
  taskers__osc133_enabled || return 0
  [[ -o prompt_percent ]] || return 0

  [[ $PS1 == *$TASKERS_OSC133_MARK_A* ]] || PS1=${TASKERS_OSC133_MARK_A}${PS1}
  [[ $PS1 == *$TASKERS_OSC133_MARK_B* ]] || PS1=${PS1}${TASKERS_OSC133_MARK_B}

  if [[ $PS1 == ${TASKERS_OSC133_MARK_A}$'\n'* ]]; then
    local rest=${PS1#${TASKERS_OSC133_MARK_A}$'\n'}
    if [[ $rest == *$'\n'* ]]; then
      PS1=${TASKERS_OSC133_MARK_A}$'\n'${rest//$'\n'/$'\n'${TASKERS_OSC133_MARK_A_SECONDARY}}
    fi
  elif [[ $PS1 == *$'\n'* ]]; then
    PS1=${PS1//$'\n'/$'\n'${TASKERS_OSC133_MARK_A_SECONDARY}}
  fi

  [[ $PS2 == *$TASKERS_OSC133_MARK_A_SECONDARY* ]] || PS2=${TASKERS_OSC133_MARK_A_SECONDARY}${PS2}
  [[ $PS2 == *$TASKERS_OSC133_MARK_B* ]] || PS2=${PS2}${TASKERS_OSC133_MARK_B}
}

taskers__osc133_unmark_prompt() {
  [[ -o prompt_percent ]] || return 0
  PS1=${PS1//$TASKERS_OSC133_MARK_A/}
  PS1=${PS1//$TASKERS_OSC133_MARK_A_SECONDARY/}
  PS1=${PS1//$TASKERS_OSC133_MARK_B/}
  PS2=${PS2//$TASKERS_OSC133_MARK_A_SECONDARY/}
  PS2=${PS2//$TASKERS_OSC133_MARK_B/}
}

taskers__osc133_print() {
  taskers__osc133_enabled || return 0
  local payload=$1
  print -rn -- "${payload}"
}

taskers__repo_root() {
  if command -v git >/dev/null 2>&1; then
    git -C "$PWD" rev-parse --show-toplevel 2>/dev/null && return 0
  fi
  if command -v jj >/dev/null 2>&1; then
    jj root 2>/dev/null || true
    return 0
  fi
  return 0
}

taskers__repo_branch() {
  if command -v git >/dev/null 2>&1; then
    git -C "$PWD" symbolic-ref --quiet --short HEAD 2>/dev/null \
      || git -C "$PWD" rev-parse --short HEAD 2>/dev/null \
      || true
    return 0
  fi
  if command -v jj >/dev/null 2>&1; then
    jj log -r @ -T 'change_id.shortest(8)' --no-graph 2>/dev/null || true
    return 0
  fi
  return 0
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

  TASKERS_META_AGENT=${TASKERS_ACTIVE_AGENT_KIND:-shell}
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

taskers__context_tty_matches() {
  local expected_tty=${TASKERS_TTY_NAME:-}
  [[ -n "$expected_tty" ]] || return 1
  local current_tty=${TTY:-}
  if [[ -z "$current_tty" ]]; then
    current_tty=$(tty 2>/dev/null || true)
  fi
  [[ "$current_tty" = /dev/* ]] || return 1
  [[ "$current_tty" = "$expected_tty" ]]
}

taskers__emit_with_metadata() {
  local kind=$1
  local message=${2:-}
  local agent_active
  local -a argv

  taskers__collect_metadata
  agent_active=$(taskers__agent_active_for_kind "$kind")

  [[ -x "${TASKERS_CTL_PATH:-}" ]] || return 0
  [[ -n "${TASKERS_WORKSPACE_ID:-}" ]] || return 0
  [[ -n "${TASKERS_PANE_ID:-}" ]] || return 0
  [[ -n "${TASKERS_SURFACE_ID:-}" ]] || return 0
  taskers__context_tty_matches || return 0

  if [[ "$kind" = "metadata" ]]; then
    argv=(
      "$TASKERS_CTL_PATH"
      signal
      --source shell
      --kind "$kind"
      --title "$TASKERS_META_TITLE"
      --cwd "$TASKERS_META_CWD"
      --agent "$TASKERS_META_AGENT"
      --agent-active "$agent_active"
    )

    [[ -n "$TASKERS_META_REPO_NAME" ]] && argv+=(--repo "$TASKERS_META_REPO_NAME")
    [[ -n "$TASKERS_META_BRANCH" ]] && argv+=(--branch "$TASKERS_META_BRANCH")
    [[ -n "${TASKERS_ACTIVE_AGENT_COMMAND:-}" ]] && argv+=(--command "$TASKERS_ACTIVE_AGENT_COMMAND")
    [[ -n "$message" ]] && argv+=(--message "$message")
  else
    local subcommand
    case "$kind" in
      started) subcommand=session-start ;;
      progress) subcommand=progress ;;
      waiting_input) subcommand=waiting ;;
      notification) subcommand=notification ;;
      completed|error) subcommand=stop ;;
      *) subcommand=active ;;
    esac

    argv=(
      "$TASKERS_CTL_PATH"
      agent-hook
      "$subcommand"
      --agent "$TASKERS_META_AGENT"
      --title "$TASKERS_META_TITLE"
    )

    [[ -n "$message" ]] && argv+=(--message "$message")
  fi

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
    && "${TASKERS_LAST_META_COMMAND:-}" = "${TASKERS_ACTIVE_AGENT_COMMAND:-}" \
    && "${TASKERS_LAST_META_AGENT_ACTIVE:-}" = "$(taskers__agent_active_for_kind metadata)" ]]; then
    return 0
  fi

  export TASKERS_LAST_META_CWD=$TASKERS_META_CWD
  export TASKERS_LAST_META_REPO_NAME=$TASKERS_META_REPO_NAME
  export TASKERS_LAST_META_BRANCH=$TASKERS_META_BRANCH
  export TASKERS_LAST_META_AGENT=$TASKERS_META_AGENT
  export TASKERS_LAST_META_TITLE=$TASKERS_META_TITLE
  export TASKERS_LAST_META_COMMAND=${TASKERS_ACTIVE_AGENT_COMMAND:-}
  export TASKERS_LAST_META_AGENT_ACTIVE=$(taskers__agent_active_for_kind metadata)
  taskers__emit_with_metadata metadata
}

taskers__invalidate_metadata_cache() {
  unset TASKERS_LAST_META_CWD
  unset TASKERS_LAST_META_REPO_NAME
  unset TASKERS_LAST_META_BRANCH
  unset TASKERS_LAST_META_AGENT
  unset TASKERS_LAST_META_TITLE
  unset TASKERS_LAST_META_COMMAND
  unset TASKERS_LAST_META_AGENT_ACTIVE
}

taskers__preexec() {
  taskers__osc133_unmark_prompt
  taskers__osc133_print $'\e]133;C\a'
  export TASKERS_OSC133_EXECUTING=1

  local agent
  agent=$(taskers__classify_command "$1" || true)
  if [[ -n "$agent" ]]; then
    export TASKERS_ACTIVE_AGENT_KIND=$agent
    export TASKERS_ACTIVE_AGENT_COMMAND=$1
    taskers__invalidate_metadata_cache
    taskers__emit_metadata_if_changed
  fi
}

taskers__precmd() {
  local exit_code=$?
  if [[ "${TASKERS_OSC133_EXECUTING:-0}" = "1" ]]; then
    taskers__osc133_print $'\e]133;D;'"${exit_code}"$'\a'
    export TASKERS_OSC133_EXECUTING=0
  fi
  taskers__osc133_mark_prompt

  if [[ -n "${TASKERS_ACTIVE_AGENT_KIND:-}" ]]; then
    unset TASKERS_ACTIVE_AGENT_KIND
    unset TASKERS_ACTIVE_AGENT_COMMAND
    taskers__invalidate_metadata_cache
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

taskers__on_chpwd() {
  taskers__emit_metadata_if_changed
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

if autoload -Uz add-zsh-hook 2>/dev/null; then
  add-zsh-hook preexec taskers__preexec
  add-zsh-hook precmd taskers__precmd
  add-zsh-hook chpwd taskers__on_chpwd
else
  typeset -ga preexec_functions
  typeset -ga precmd_functions
  typeset -ga chpwd_functions
  preexec_functions+=(taskers__preexec)
  precmd_functions+=(taskers__precmd)
  chpwd_functions+=(taskers__on_chpwd)
fi
taskers__normalize_backspace
if [[ -z "${TASKERS_TTY_NAME:-}" ]]; then
  TASKERS_TTY_NAME=$(tty 2>/dev/null || true)
  [[ "$TASKERS_TTY_NAME" = /dev/* ]] || unset TASKERS_TTY_NAME
  [[ -n "${TASKERS_TTY_NAME:-}" ]] && export TASKERS_TTY_NAME
fi
taskers__emit_metadata_if_changed
