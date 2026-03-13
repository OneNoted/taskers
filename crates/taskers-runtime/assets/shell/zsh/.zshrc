taskers_user_zdotdir="${TASKERS_USER_ZDOTDIR:-$HOME}"
if [ "${TASKERS_SHELL_PROFILE:-default}" = "clean" ]; then
  PROMPT='taskers %# '
else
  export ZDOTDIR="$taskers_user_zdotdir"
  if [ -f "$taskers_user_zdotdir/.zshrc" ]; then
    . "$taskers_user_zdotdir/.zshrc"
  fi
fi

if [ -f "${TASKERS_SHELL_INTEGRATION_DIR}/taskers-hooks.zsh" ]; then
  . "${TASKERS_SHELL_INTEGRATION_DIR}/taskers-hooks.zsh"
fi

unset taskers_user_zdotdir
