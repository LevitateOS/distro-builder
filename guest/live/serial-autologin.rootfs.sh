#!/bin/sh
echo "@SHELL_READY_MARKER@"
echo "@SHELL_READY_MARKER@" >/dev/console 2>/dev/null || true
echo "@SHELL_READY_MARKER@" >/dev/kmsg 2>/dev/null || true
exec /bin/sh -l
