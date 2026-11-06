#!/bin/sh
# Autologin for serial console testing
# Called by agetty -l as the login program
# agetty has already set up stdin/stdout/stderr on the tty

# Optional boot-injection environment (e.g. SSH_AUTHORIZED_KEY, LEVITATE_INSTALL_SERIAL_UX).
# Parse as literal KEY=VALUE lines to avoid executing payload content.
@BOOT_INJECTION_IMPORT@

echo "[autologin] Starting login shell..."
echo "@SHELL_READY_MARKER@"
echo "[autologin] Starting login shell..." >/dev/console 2>/dev/null || true
echo "@SHELL_READY_MARKER@" >/dev/console 2>/dev/null || true
echo "@SHELL_READY_MARKER@" >/dev/kmsg 2>/dev/null || true

if [ "${LEVITATE_INSTALL_SERIAL_UX:-0}" = "1" ] && [ -x @INSTALL_ENTRYPOINT@ ]; then
    echo "[autologin] Launching install UX on serial console..."
    export LEVITATE_INSTALL_UX_LAUNCHED=1
    exec @INSTALL_ENTRYPOINT@
fi

# Run sh as login shell (sources /etc/profile and /etc/profile.d/*)
# In Alpine, /bin/sh is busybox ash
exec /bin/sh -l
