taskers_user_zdotdir="${TASKERS_USER_ZDOTDIR:-$HOME}"
taskers_wrapper_zdotdir="${ZDOTDIR:-$taskers_user_zdotdir}"

if [ "${TASKERS_SHELL_PROFILE:-default}" != "clean" ] && [ -f "$taskers_user_zdotdir/.zshenv" ]; then
  export ZDOTDIR="$taskers_user_zdotdir"
  . "$taskers_user_zdotdir/.zshenv"
  export ZDOTDIR="$taskers_wrapper_zdotdir"
fi

unset taskers_user_zdotdir
unset taskers_wrapper_zdotdir
