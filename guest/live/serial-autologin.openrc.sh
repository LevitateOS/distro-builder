#!/bin/sh
# The serial console's autologin where no install screen runs on it:
# busybox getty -n -l starts this in place of login, on the serial line.
echo "[autologin] Starting login shell..."
echo "@SHELL_READY_MARKER@"
echo "[autologin] Starting login shell..." >/dev/console 2>/dev/null || true
echo "@SHELL_READY_MARKER@" >/dev/console 2>/dev/null || true
echo "@SHELL_READY_MARKER@" >/dev/kmsg 2>/dev/null || true

# busybox ash as a login shell (sources /etc/profile and /etc/profile.d/*)
exec /bin/sh -l
