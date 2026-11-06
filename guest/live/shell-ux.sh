#!/bin/sh
# Live shell defaults: editor, pager, colours, prompt and bash history.
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

export EDITOR="${EDITOR:-vi}"
export VISUAL="${VISUAL:-$EDITOR}"
export PAGER="${PAGER:-less}"
export LESS="${LESS:--FRSX}"

if [ -z "${NO_COLOR:-}" ]; then
    export CLICOLOR="${CLICOLOR:-1}"
    if command -v dircolors >/dev/null 2>&1; then
        eval "$(dircolors -b 2>/dev/null || true)"
    fi
    alias ls='ls --color=auto'
    alias ll='ls -alF --color=auto'
    alias la='ls -A --color=auto'
    alias grep='grep --color=auto'
    alias egrep='grep -E --color=auto'
    alias fgrep='grep -F --color=auto'
fi

if [ -n "${BASH_VERSION:-}" ]; then
    export HISTCONTROL="${HISTCONTROL:-ignoredups:erasedups}"
    export HISTSIZE="${HISTSIZE:-10000}"
    export HISTFILESIZE="${HISTFILESIZE:-20000}"
    export HISTTIMEFORMAT="${HISTTIMEFORMAT:-%F %T }"
    shopt -s histappend 2>/dev/null || true
    shopt -s checkwinsize 2>/dev/null || true
    if [ -r /usr/share/bash-completion/bash_completion ]; then
        . /usr/share/bash-completion/bash_completion
    elif [ -r /etc/bash_completion ]; then
        . /etc/bash_completion
    fi
    if [ -z "${NO_COLOR:-}" ]; then
        PS1="\[\033[1;32m\]\u@\h\[\033[0m\]:\[\033[1;34m\]\w\[\033[0m\]\\$ "
    else
        PS1='[live \u@\h \W]\$ '
    fi
elif [ -z "${PS1:-}" ]; then
    PS1='[live \u@\h \W]\$ '
fi

if [ "${LIVE_HELP_HINT_SHOWN:-0}" != "1" ]; then
    export LIVE_HELP_HINT_SHOWN=1
    echo "[live] run 'live-help' for common commands."
fi
