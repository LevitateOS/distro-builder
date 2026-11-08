#!/bin/sh
# install-ux: the install screen. getty starts it in place of login on the
# install screen's consoles. When the screen ends, for any reason, a root
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
    exec @SPLIT@ --smoke --left @LEFT@ --right @DOCS_ARGV@
fi

# A serial console runs the screen only when the boot asks for it; a boot
# without boot injection has no payload file.
console=$(tty)
for serial in @SERIAL_CONSOLES@; do
    if [ "$console" = "$serial" ]; then
        [ -f @BOOT_INJECTION_ENV@ ] && grep -qx '@SERIAL_KEY@=1' @BOOT_INJECTION_ENV@ ||
            exec "$SHELL" -l
    fi
done

@SPLIT@ --left @LEFT@ --right @DOCS_ARGV@ ||
    echo "The install screen stopped with an error; starting a shell."
exec "$SHELL" -l
