#!/bin/sh
set -eu

DOCS_FILE="@DOCS_TEXT@"
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

if [ -f "$DOCS_FILE" ]; then
    if [ "${LEVITATE_DOCS_PLAIN:-0}" != "1" ] && command -v less >/dev/null 2>&1; then
        less "$DOCS_FILE" || true
        echo "Install guide closed. Run 'levitate-install-docs' to reopen it."
    else
        cat "$DOCS_FILE"
    fi
else
    echo "install docs payload missing at $DOCS_FILE"
fi

exec "${SHELL:-@SHELL@}" -l
