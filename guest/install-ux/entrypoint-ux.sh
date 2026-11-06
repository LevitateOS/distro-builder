#!/bin/sh
set -eu

choose_install_helper() {
    if [ -x /usr/local/bin/@SPLIT@ ]; then
        printf '%s\n' "/usr/local/bin/@SPLIT@"
        return 0
    fi
    if command -v @SPLIT@ >/dev/null 2>&1; then
        printf '%s\n' "@SPLIT@"
        return 0
    fi
    if command -v levitate-install-docs >/dev/null 2>&1; then
        printf '%s\n' "levitate-install-docs"
        return 0
    fi
    return 1
}

if [ "${1:-}" = "--probe" ]; then
    helper="$(choose_install_helper || true)"
    if [ -n "$helper" ]; then
        printf '@PROBE@%s\n' "$helper"
        exit 0
    fi
    printf '@PROBE@none\n'
    exit 3
fi

echo "[@DISTRO@] install experience: UX"
echo "Starting local interactive install helper if available..."

helper="$(choose_install_helper || true)"
if [ -n "$helper" ]; then
    case "$helper" in
        */@SPLIT@|@SPLIT@)
            if [ "${@SMOKE@:-0}" = "1" ]; then
                exec "$helper" --smoke
            fi
            ;;
    esac
    exec "$helper"
fi

echo "No local docs TUI binary found; dropping to shell."
exec "${SHELL:-@SHELL@}" -l
