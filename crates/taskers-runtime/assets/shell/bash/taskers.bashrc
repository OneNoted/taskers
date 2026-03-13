[ -n "${TASKERS_BASH_RC_LOADED:-}" ] && return 0
export TASKERS_BASH_RC_LOADED=1

if [ "${TASKERS_SHELL_PROFILE:-default}" = "clean" ]; then
  export PS1='taskers$ '
else
  if [ -f /etc/bash.bashrc ]; then
    . /etc/bash.bashrc
  fi

  if [ -f "${TASKERS_USER_BASHRC:-$HOME/.bashrc}" ]; then
    . "${TASKERS_USER_BASHRC:-$HOME/.bashrc}"
  fi
fi

if [ -f "${TASKERS_SHELL_INTEGRATION_DIR}/taskers-hooks.bash" ]; then
  . "${TASKERS_SHELL_INTEGRATION_DIR}/taskers-hooks.bash"
fi
