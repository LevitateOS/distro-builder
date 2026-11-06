#!/bin/sh
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

[ -n "${TMUX:-}" ] && return 0
[ "${LEVITATE_INSTALL_UX_LAUNCHED:-0}" = "1" ] && return 0

@BOOT_INJECTION_IMPORT@

TTY="$(tty 2>/dev/null || true)"
if [ -z "${LEVITATE_INSTALL_LEFT_CMD:-}" ]; then
    if [ -x /bin/bash ]; then
        LEVITATE_INSTALL_LEFT_CMD="/bin/bash -il"
    else
        LEVITATE_INSTALL_LEFT_CMD="/bin/sh -il"
    fi
    export LEVITATE_INSTALL_LEFT_CMD
fi
if [ "$TTY" = "/dev/tty1" ]; then
    :
elif [ "$TTY" = "/dev/ttyS0" ] && [ "${LEVITATE_INSTALL_SERIAL_UX:-0}" = "1" ]; then
    if [ -z "${LEVITATE_INSTALL_RIGHT_CMD:-}" ]; then
        LEVITATE_INSTALL_RIGHT_CMD="docs-tui --slug installation"
        export LEVITATE_INSTALL_RIGHT_CMD
    fi
    export LEVITATE_DOCS_PLAIN=1
    :
else
    return 0
fi

echo "[install-ux] Launching install UX on $TTY..."
export LEVITATE_INSTALL_UX_LAUNCHED=1
exec @INSTALL_ENTRYPOINT@
