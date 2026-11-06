#!/bin/sh
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

[ -n "${NO_COLOR:-}" ] && return 0

export CLICOLOR="${CLICOLOR:-1}"
export LESS="${LESS:--FRSX}"
export PAGER="${PAGER:-less}"

if command -v dircolors >/dev/null 2>&1; then
    eval "$(dircolors -b 2>/dev/null || true)"
fi

if command -v ls >/dev/null 2>&1; then
    alias ls='ls --color=auto'
    alias ll='ls -alF --color=auto'
    alias la='ls -A --color=auto'
fi

if command -v grep >/dev/null 2>&1; then
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
    export PS1="\[\033[1;32m\]\u@\h\[\033[0m\]:\[\033[1;34m\]\w\[\033[0m\]\\$ "
fi
