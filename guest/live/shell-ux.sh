#!/bin/sh
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

export EDITOR="${EDITOR:-vi}"
export VISUAL="${VISUAL:-$EDITOR}"
export PAGER="${PAGER:-less}"
export LESS="${LESS:--FRSX}"

if [ -z "${PS1:-}" ]; then
    export PS1='[live \u@\h \W]\$ '
fi

if [ "${LIVE_HELP_HINT_SHOWN:-0}" != "1" ]; then
    export LIVE_HELP_HINT_SHOWN=1
    echo "[live] run 'live-help' for common commands."
fi
