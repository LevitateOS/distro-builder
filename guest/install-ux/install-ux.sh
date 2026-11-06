#!/bin/sh
# install-ux: the install screen. getty starts it in place of login on the
# consoles the variant lists. When the screen ends, for any reason, a root
# login shell takes over; when that shell exits, getty starts the screen again.
export HOME=/root USER=root LOGNAME=root SHELL=@SHELL@
cd "$HOME" 2>/dev/null || cd /

if [ "${1:-}" = "--probe" ]; then
    if [ -x @SPLIT@ ]; then
        printf '@PROBE@%s\n' "@SPLIT@"
        exit 0
    fi
    printf '@PROBE@none\n'
    exit 3
fi

if [ "${@SMOKE@:-0}" = "1" ]; then
    exec @SPLIT@ --smoke --left "$SHELL" -l --right @DOCS_ARGV@
fi

case "$(tty 2>/dev/null)" in
    /dev/ttyS*)
        # The serial console runs the screen only when the boot asks for it.
        grep -qx '@SERIAL_KEY@=1' @BOOT_INJECTION_ENV@ 2>/dev/null || exec "$SHELL" -l
        ;;
esac

@SPLIT@ --left "$SHELL" -l --right @DOCS_ARGV@ ||
    echo "The install screen stopped with an error; starting a shell."
exec "$SHELL" -l
