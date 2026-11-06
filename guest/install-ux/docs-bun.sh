#!/bin/sh
set -eu

APP="@DOCS_BUNDLE@"
if [ ! -r "$APP" ]; then
    echo "install docs bundle missing at $APP" >&2
    exit 1
fi

case "${TERM:-}" in
    ""|dumb|vt100|vt102|linux)
        export TERM=xterm-256color
        ;;
esac
export COLORTERM="${COLORTERM:-truecolor}"
export FORCE_COLOR="${FORCE_COLOR:-3}"
if [ -n "${NO_COLOR:-}" ]; then
    unset NO_COLOR
fi

exec @BUN@ "$APP" "$@"
