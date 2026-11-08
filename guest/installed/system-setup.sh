#!/bin/sh
# Installed by the system recipe. recsetup validates choices and invokes this
# target-side policy; no compiler image or build runtime is needed on the live ISO.
# What the system is comes from the distro profile the builder writes into
# /etc/os-release (REC_* fields), not from probing for programs.
set -eu

missing_profile() {
    echo "/etc/os-release has no $1: the system was built without a distro profile (rebuild it with the current builder)" >&2
    exit 2
}
REC_INIT='' REC_PACKAGES=''
[ -r /etc/os-release ] || missing_profile REC_INIT
. /etc/os-release
init=$REC_INIT packages=$REC_PACKAGES
[ -n "$init" ] || missing_profile REC_INIT
[ -n "$packages" ] || missing_profile REC_PACKAGES

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
        else echo "time zone is unavailable: $2" >&2; exit 2; fi
        ;;
    locale)
        # recsetup offers only locales usable as they are (glibc's compiled ones,
        # musl's), so setting LANG is all there is. systemd reads locale.conf
        # itself; on OpenRC the recipe's /etc/profile.d/locale.sh exports it.
        assignment /etc/locale.conf LANG "$2"
        ;;
    keymap)
        case "$init" in
            systemd) assignment /etc/vconsole.conf KEYMAP "$2" ;;
            openrc)
                # busybox's binary keymaps (kbd-bkeymaps), loaded by busybox-openrc's loadkmap.
                bmap=$(find /usr/share/bkeymaps -name "$2.bmap.gz" | head -n 1)
                if [ -z "$bmap" ]; then echo "keymap is unavailable: $2" >&2; exit 2; fi
                assignment /etc/conf.d/loadkmap KEYMAP "\"$bmap\""
                rc-update add loadkmap boot
                ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
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
        # The account tool is the base distro's: shadow-utils' useradd on the
        # RPM (Fedora) base, busybox's adduser on the APK (Alpine) base.
        case "$packages" in
            rpm)
                if [ -n "$groups" ]; then useradd -m -s "$shell" -G "$groups" "$name"
                else useradd -m -s "$shell" "$name"; fi
                ;;
            apk)
                adduser -D -s "$shell" "$name"
                old_ifs=$IFS; IFS=,
                for group in $groups; do addgroup "$name" "$group"; done
                IFS=$old_ifs
                ;;
            *) echo "unsupported package backend: $packages" >&2; exit 2 ;;
        esac
        ;;
    machine-id)
        case "$init" in
            systemd) systemd-machine-id-setup ;;
            openrc)
                id=
                if [ -e /etc/machine-id ]; then id=$(cat /etc/machine-id); fi
                case "$id" in *[!0-9a-f]*|'') valid=no ;; *) valid=yes ;; esac
                if [ "$valid" = yes ] && [ "${#id}" -eq 32 ]; then exit 0; fi
                od -An -N16 -tx1 /dev/urandom | tr -d ' \n' > /etc/machine-id
                printf '\n' >> /etc/machine-id
                chmod 0444 /etc/machine-id
                ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    service)
        case "$init" in
            systemd) SYSTEMD_OFFLINE=1 systemctl enable "$2" ;;
            openrc) rc-update add "$2" default ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    *) echo "usage: recipe-system-setup timezone|locale|keymap|hostname|user|machine-id|service ..." >&2; exit 2 ;;
esac
