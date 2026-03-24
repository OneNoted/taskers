[ -n "${TASKERS_HOOKS_BASH_LOADED:-}" ] && return 0
export TASKERS_HOOKS_BASH_LOADED=1

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
    codex) printf '%s' 'codex' ;;
    claude|claude-code) printf '%s' 'claude' ;;
    opencode) printf '%s' 'opencode' ;;
    aider) printf '%s' 'aider' ;;
    *) return 1 ;;
  esac
}

taskers__classify_command() {
  set -f
  # shellcheck disable=SC2086
  set -- $1
  set +f

  while [ $# -gt 0 ]; do
    case "$1" in
      *=*)
        shift
        ;;
      env)
        shift
        ;;
      npx|pnpx|bunx|uvx)
        shift
        [ $# -gt 0 ] || return 1
        taskers__classify_token "$1"
        return $?
        ;;
      pnpm|yarn)
        shift
        if [ $# -gt 0 ] && [ "$1" = "dlx" ]; then
          shift
          [ $# -gt 0 ] || return 1
          taskers__classify_token "$1"
          return $?
        fi
        return 1
        ;;
      *)
        taskers__classify_token "$1"
        return $?
        ;;
    esac
  done

  return 1
}

taskers__collect_metadata() {
  TASKERS_META_CWD=$PWD
  TASKERS_META_REPO_ROOT=$(taskers__repo_root)
  if [ -n "$TASKERS_META_REPO_ROOT" ]; then
    TASKERS_META_REPO_NAME=${TASKERS_META_REPO_ROOT##*/}
    [ -n "$TASKERS_META_REPO_NAME" ] || TASKERS_META_REPO_NAME=/
    TASKERS_META_BRANCH=$(taskers__repo_branch)
  else
    TASKERS_META_REPO_NAME=
    TASKERS_META_BRANCH=
  fi

  TASKERS_META_AGENT=${TASKERS_ACTIVE_AGENT_KIND:-${TASKERS_PANE_AGENT_KIND:-shell}}
  TASKERS_META_LABEL=$TASKERS_META_REPO_NAME
  if [ -z "$TASKERS_META_LABEL" ]; then
    TASKERS_META_LABEL=${PWD##*/}
    [ -n "$TASKERS_META_LABEL" ] || TASKERS_META_LABEL=/
  fi

  if [ "$TASKERS_META_AGENT" = "shell" ]; then
    TASKERS_META_TITLE=$TASKERS_META_LABEL
  else
    TASKERS_META_TITLE="${TASKERS_META_AGENT} :: ${TASKERS_META_LABEL}"
  fi
}

taskers__agent_active_for_kind() {
  case "$1" in
    started|progress|waiting_input)
      printf '%s' '1'
      ;;
    completed|error)
      printf '%s' '0'
      ;;
    *)
      if [ -n "${TASKERS_ACTIVE_AGENT_KIND:-}" ]; then
        printf '%s' '1'
      else
        printf '%s' '0'
      fi
      ;;
  esac
}

taskers__context_tty_matches() {
  local expected_tty=${TASKERS_TTY_NAME:-}
  local current_tty
  [ -n "$expected_tty" ] || return 1
  current_tty=$(tty 2>/dev/null || true)
  case "$current_tty" in
    /dev/*) ;;
    *) return 1 ;;
  esac
  [ "$current_tty" = "$expected_tty" ]
}

taskers__emit_with_metadata() {
  local kind=$1
  local message=${2:-}
  local agent_active
  local -a argv

  taskers__collect_metadata
  agent_active=$(taskers__agent_active_for_kind "$kind")

  [ -x "${TASKERS_CTL_PATH:-}" ] || return 0
  [ -n "${TASKERS_WORKSPACE_ID:-}" ] || return 0
  [ -n "${TASKERS_PANE_ID:-}" ] || return 0
  [ -n "${TASKERS_SURFACE_ID:-}" ] || return 0
  taskers__context_tty_matches || return 0

  if [ "$kind" = "metadata" ]; then
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

    if [ -n "$TASKERS_META_REPO_NAME" ]; then
      argv+=(--repo "$TASKERS_META_REPO_NAME")
    fi
    if [ -n "$TASKERS_META_BRANCH" ]; then
      argv+=(--branch "$TASKERS_META_BRANCH")
    fi
    if [ -n "$message" ]; then
      argv+=(--message "$message")
    fi
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

    if [ -n "$message" ]; then
      argv+=(--message "$message")
    fi
  fi

  (
    exec </dev/null
    "${argv[@]}"
  ) >/dev/null 2>&1 &
}

taskers__emit_metadata_if_changed() {
  local agent_active
  taskers__collect_metadata
  agent_active=$(taskers__agent_active_for_kind metadata)

  if [ "${TASKERS_LAST_META_CWD:-}" = "$TASKERS_META_CWD" ] \
    && [ "${TASKERS_LAST_META_REPO_NAME:-}" = "$TASKERS_META_REPO_NAME" ] \
    && [ "${TASKERS_LAST_META_BRANCH:-}" = "$TASKERS_META_BRANCH" ] \
    && [ "${TASKERS_LAST_META_AGENT:-}" = "$TASKERS_META_AGENT" ] \
    && [ "${TASKERS_LAST_META_TITLE:-}" = "$TASKERS_META_TITLE" ] \
    && [ "${TASKERS_LAST_META_AGENT_ACTIVE:-}" = "$agent_active" ]; then
    return 0
  fi

  export TASKERS_LAST_META_CWD=$TASKERS_META_CWD
  export TASKERS_LAST_META_REPO_NAME=$TASKERS_META_REPO_NAME
  export TASKERS_LAST_META_BRANCH=$TASKERS_META_BRANCH
  export TASKERS_LAST_META_AGENT=$TASKERS_META_AGENT
  export TASKERS_LAST_META_TITLE=$TASKERS_META_TITLE
  export TASKERS_LAST_META_AGENT_ACTIVE=$agent_active
  taskers__emit_with_metadata metadata
}

taskers__preexec() {
  local agent
  agent=$(taskers__classify_command "$1" || true)
  if [ -n "$agent" ]; then
    export TASKERS_PANE_AGENT_KIND=$agent
    export TASKERS_ACTIVE_AGENT_KIND=$agent
    taskers__emit_with_metadata started
  fi
}

taskers__precmd() {
  local status=$1
  if [ -n "${TASKERS_ACTIVE_AGENT_KIND:-}" ]; then
    if [ "$status" -eq 0 ]; then
      taskers__emit_with_metadata completed
    else
      taskers__emit_with_metadata error "Exited with status $status"
    fi
    unset TASKERS_ACTIVE_AGENT_KIND
  fi

  taskers__emit_metadata_if_changed
}

taskers_signal() {
  [ $# -gt 0 ] || return 1
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

taskers__prompt_command() {
  local status=$?
  taskers__precmd "$status"
  TASKERS_AT_PROMPT=1
  return $status
}

taskers__preexec_invoke_exec() {
  case "${BASH_COMMAND:-}" in
    taskers__prompt_command*|taskers__preexec_invoke_exec*|taskers__emit_with_metadata*|taskers__emit_metadata_if_changed*)
      return 0
      ;;
  esac

  [ "${TASKERS_AT_PROMPT:-1}" = "1" ] || return 0
  TASKERS_AT_PROMPT=0
  taskers__preexec "${BASH_COMMAND:-}"
}

TASKERS_AT_PROMPT=1
trap 'taskers__preexec_invoke_exec' DEBUG
PROMPT_COMMAND="taskers__prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
if [ -z "${TASKERS_TTY_NAME:-}" ]; then
  TASKERS_TTY_NAME=$(tty 2>/dev/null || true)
  case "$TASKERS_TTY_NAME" in
    /dev/*) export TASKERS_TTY_NAME ;;
    *) unset TASKERS_TTY_NAME ;;
  esac
fi
taskers__emit_metadata_if_changed
