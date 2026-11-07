#!/bin/sh
# Installed by the system recipe. recsetup validates choices and invokes this
# target-side policy; no compiler image or build runtime is needed on the live ISO.
set -eu
init='@INIT@'
libc='@LIBC@'

assignment() {
    file=$1 key=$2 value=$3
    mkdir -p "$(dirname "$file")"
    tmp=$(mktemp "${file}.XXXXXX")
    if [ -f "$file" ]; then
        awk -v key="$key" -v value="$value" '
            $0 ~ "^[[:space:]]*" key "=" { if (!done++) print key "=" value; next }
            { print }
            END { if (!done) print key "=" value }
        ' "$file" > "$tmp"
    else
        printf '%s=%s\n' "$key" "$value" > "$tmp"
    fi
    chmod 0644 "$tmp"
    mv -f "$tmp" "$file"
}

case "${1:-}" in
    timezone)
        if [ -f "/usr/share/zoneinfo/$2" ]; then
            ln -sfn "../usr/share/zoneinfo/$2" /etc/localtime
        elif [ "$2" = UTC ]; then rm -f /etc/localtime
        else echo "time zone is unavailable: $2" >&2; exit 2; fi
        ;;
    locale)
        lang=$2 method=$3
        case "$method" in
            localedef) localedef -i "$4" -f "$5" "$lang" ;;
            locale_gen)
                line=$4
                tmp=$(mktemp /etc/locale.gen.XXXXXX)
                awk -v wanted="$line" '
                    { bare=$0; sub(/^[[:space:]#]+/, "", bare) }
                    bare == wanted { if (!done++) print wanted; next }
                    { print }
                    END { if (!done) print wanted }
                ' /etc/locale.gen > "$tmp"
                chmod 0644 "$tmp"; mv -f "$tmp" /etc/locale.gen
                locale-gen
                ;;
            precompiled|musl) ;;
            *) echo "unsupported locale method: $method" >&2; exit 2 ;;
        esac
        assignment /etc/locale.conf LANG "$lang"
        # The musl login-shell export is installed by the recipe.
        ;;
    keymap)
        if [ "$init" = systemd ]; then assignment /etc/vconsole.conf KEYMAP "$2"
        else
            # busybox's binary keymaps (kbd-bkeymaps), loaded by busybox-openrc's loadkmap;
            # otherwise kbd's, loaded by its keymaps (or loadkeys) service.
            bmap=$(find /usr/share/bkeymaps -name "$2.bmap.gz" 2>/dev/null | head -n 1)
            if [ -n "$bmap" ] && [ -f /etc/init.d/loadkmap ]; then
                assignment /etc/conf.d/loadkmap KEYMAP "\"$bmap\""
                rc-update add loadkmap boot
            else
                service=keymaps
                if [ ! -f /etc/init.d/keymaps ] && [ -f /etc/init.d/loadkeys ]; then service=loadkeys; fi
                assignment "/etc/conf.d/$service" keymap "\"$2\""
                if [ -f "/etc/init.d/$service" ]; then rc-update add "$service" boot; fi
            fi
        fi
        ;;
    hostname)
        printf '%s\n' "$2" > /etc/hostname
        tmp=$(mktemp /etc/hosts.XXXXXX)
        if [ -f /etc/hosts ]; then
            awk '$1 != "127.0.1.1" { print }' /etc/hosts > "$tmp"
        fi
        if ! awk '$1=="127.0.0.1" { for(i=2;i<=NF;i++) if($i=="localhost") found=1 } END { exit !found }' "$tmp"; then
            printf '127.0.0.1 localhost\n' >> "$tmp"
        fi
        if ! awk '$1=="::1" { for(i=2;i<=NF;i++) if($i=="localhost") found=1 } END { exit !found }' "$tmp"; then
            printf '::1 localhost\n' >> "$tmp"
        fi
        printf '127.0.1.1 %s.localdomain %s\n' "$2" "$2" >> "$tmp"
        chmod 0644 "$tmp"; mv -f "$tmp" /etc/hosts
        ;;
    user)
        name=$2 shell=$3 groups=$4
        if command -v useradd >/dev/null 2>&1; then
            if [ -n "$groups" ]; then useradd -m -s "$shell" -G "$groups" "$name"
            else useradd -m -s "$shell" "$name"; fi
        else
            adduser -D -s "$shell" "$name"
            old_ifs=$IFS; IFS=,
            for group in $groups; do addgroup "$name" "$group"; done
            IFS=$old_ifs
        fi
        ;;
    machine-id)
        if command -v systemd-machine-id-setup >/dev/null 2>&1; then systemd-machine-id-setup
        else
            id=$(cat /etc/machine-id 2>/dev/null || true)
            case "$id" in *[!0-9a-f]*|'') valid=no ;; *) valid=yes ;; esac
            if [ "$valid" = yes ] && [ "${#id}" -eq 32 ]; then exit 0; fi
            od -An -N16 -tx1 /dev/urandom | tr -d ' \n' > /etc/machine-id
            printf '\n' >> /etc/machine-id
            chmod 0444 /etc/machine-id
        fi
        ;;
    service)
        if [ "$init" = systemd ]; then SYSTEMD_OFFLINE=1 systemctl enable "$2"
        else rc-update add "$2" default; fi
        ;;
    *) echo "usage: recipe-system-setup timezone|locale|keymap|hostname|user|machine-id|service ..." >&2; exit 2 ;;
esac
