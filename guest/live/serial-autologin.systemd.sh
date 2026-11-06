#!/bin/sh
# Live override: use portable C locale (always available in minimal rootfs).
export LANG=C
unset LC_ALL LC_CTYPE LC_NUMERIC LC_TIME LC_COLLATE LC_MESSAGES \
      LC_MONETARY LC_PAPER LC_NAME LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT \
      LC_IDENTIFICATION

# Optional boot-injection environment (e.g. SSH_AUTHORIZED_KEY, LEVITATE_INSTALL_SERIAL_UX).
# Parse as literal KEY=VALUE lines to avoid executing payload content.
@BOOT_INJECTION_IMPORT@

echo "@SHELL_READY_MARKER@"

# Keep default serial shell readable, but allow explicit verbose override for debugging.
if [ "${LEVITATE_INSTALL_SERIAL_VERBOSE:-0}" = "1" ]; then
    if [ -w /proc/sys/kernel/printk ]; then
        echo 7 >/proc/sys/kernel/printk 2>/dev/null || true
    fi
    if command -v dmesg >/dev/null 2>&1; then
        dmesg -n 7 >/dev/null 2>&1 || true
    fi
else
    if [ -w /proc/sys/kernel/printk ]; then
        echo 1 >/proc/sys/kernel/printk 2>/dev/null || true
    fi
    if command -v dmesg >/dev/null 2>&1; then
        dmesg -n 1 >/dev/null 2>&1 || true
    fi
fi

if [ "${LEVITATE_INSTALL_SERIAL_UX:-0}" = "1" ] && [ -x @INSTALL_ENTRYPOINT@ ]; then
    echo "[autologin] Launching install UX on serial console..."
    export LEVITATE_INSTALL_UX_LAUNCHED=1
    exec @INSTALL_ENTRYPOINT@
fi

exec /bin/bash -il
