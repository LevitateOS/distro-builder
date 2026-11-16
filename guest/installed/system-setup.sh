#!/bin/sh
# The target-side setup program (installed-system.rhai installs it as
# /usr/libexec/recipe-system-setup; recsetup runs it inside the target). It
# applies one machine choice per call, by the mechanism of the system's init
# and package backend, which it reads from /etc/os-release (REC_* fields).
set -eu

missing_profile() {
    echo "/etc/os-release has no $1: the system was built without a distro profile (rebuild it with the current builder)" >&2
    exit 2
}
REC_INIT='' REC_PACKAGES='' REC_DESKTOP=''
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
        # Setting LANG is all there is: systemd reads locale.conf itself, and
        # on OpenRC the recipe's /etc/profile.d/locale.sh exports it.
        assignment /etc/locale.conf LANG "$2"
        ;;
    keymap)
        case "$init" in
            systemd) assignment /etc/vconsole.conf KEYMAP "$2" ;;
            openrc)
                # kbd's keymaps, loaded by the kbd-openrc loadkeys service.
                if [ -z "$(find /usr/share/keymaps -name "$2.map.gz" | head -n 1)" ]; then echo "keymap is unavailable: $2" >&2; exit 2; fi
                assignment /etc/conf.d/loadkeys keymap "\"$2\""
                rc-update add loadkeys boot
                ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    x11-keyboard)
        # A desktop's keyboard layout ($2, variant $3): the X server's
        # and the login screen's (xorg.conf.d, which systemd-localed also
        # reads), and on Plasma the default of every user's session.
        layout=$2 variant=${3:-}
        mkdir -p /etc/X11/xorg.conf.d
        conf=/etc/X11/xorg.conf.d/00-keyboard.conf
        printf 'Section "InputClass"\n    Identifier "system-keyboard"\n    MatchIsKeyboard "on"\n    Option "XkbLayout" "%s"\n' "$layout" > "$conf"
        if [ -n "$variant" ]; then printf '    Option "XkbVariant" "%s"\n' "$variant" >> "$conf"; fi
        printf 'EndSection\n' >> "$conf"
        chmod 0644 "$conf"
        if [ "$REC_DESKTOP" = plasma ]; then
            mkdir -p /etc/xdg
            printf '[Layout]\nLayoutList=%s\nUse=true\n' "$layout" > /etc/xdg/kxkbrc
            if [ -n "$variant" ]; then printf 'VariantList=%s\n' "$variant" >> /etc/xdg/kxkbrc; fi
            chmod 0644 /etc/xdg/kxkbrc
        fi
        ;;
    autologin)
        # Log user $2 in to session $3 at boot without asking: reclogin, the
        # login screen, reads it from its settings file (the other lines of an
        # existing file are kept; a key already there is replaced in place, a
        # missing one is added).
        file=/etc/reclogin.conf
        tmp=$(mktemp "${file}.XXXXXX")
        if [ -f "$file" ]; then
            awk -v user="$2" -v session="$3" '
                /^[[:space:]]*autologin[[:space:]]*=/ { if (!u++) print "autologin = " user; next }
                /^[[:space:]]*autologin_session[[:space:]]*=/ { if (!s++) print "autologin_session = " session; next }
                { print }
                END {
                    if (!u) print "autologin = " user
                    if (!s) print "autologin_session = " session
                }
            ' "$file" > "$tmp"
        else
            printf 'autologin = %s\nautologin_session = %s\n' "$2" "$3" > "$tmp"
        fi
        chmod 0644 "$tmp"
        mv "$tmp" "$file"
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
        # $5 (optional): groups the user joins where the system has them
        # (a desktop session's device groups, which the software it got makes).
        name=$2 shell=$3 groups=$4 optional=${5:-}
        old_ifs=$IFS; IFS=,
        for group in $optional; do
            if grep -q "^$group:" /etc/group; then groups=${groups:+$groups,}$group; fi
        done
        IFS=$old_ifs
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
        # A home is its owner's alone on both systems. busybox's adduser makes
        # it readable by everyone; useradd follows login.defs, which may change.
        home=$(awk -F: -v user="$name" '$1 == user { print $6 }' /etc/passwd)
        if [ -n "$home" ] && [ -d "$home" ]; then chmod 0700 "$home"; fi
        ;;
    machine-id)
        id=
        if [ -r /etc/machine-id ]; then id=$(cat /etc/machine-id); fi
        case "$id" in *[!0-9a-f]*|''|00000000000000000000000000000000) valid=no ;; *) valid=yes ;; esac
        if [ "$valid" = yes ] && [ "${#id}" -eq 32 ]; then exit 0; fi
        if [ ! -w /etc ]; then
            echo "machine identity must be seeded before immutable boot" >&2
            exit 1
        fi
        case "$init" in
            systemd) systemd-machine-id-setup ;;
            openrc)
                id=
                if [ -e /etc/machine-id ]; then id=$(cat /etc/machine-id); fi
                case "$id" in *[!0-9a-f]*|''|00000000000000000000000000000000) valid=no ;; *) valid=yes ;; esac
                if [ "$valid" = yes ] && [ "${#id}" -eq 32 ]; then exit 0; fi
                od -An -N16 -tx1 /dev/urandom | tr -d ' \n' > /etc/machine-id
                printf '\n' >> /etc/machine-id
                chmod 0444 /etc/machine-id
                ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    host-keys)
        case "$init" in
            systemd) [ ! -e /etc/systemd/system/multi-user.target.wants/sshd.service ] || ssh-keygen -A ;;
            openrc) [ ! -e /etc/runlevels/default/sshd ] || ssh-keygen -A ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    service)
        case "$2" in sshd|sshd.service) ssh-keygen -A || exit 1 ;; esac
        case "$init" in
            systemd) SYSTEMD_OFFLINE=1 systemctl enable "$2" ;;
            openrc) rc-update add "$2" default ;;
            *) echo "unsupported init system: $init" >&2; exit 2 ;;
        esac
        ;;
    *) echo "usage: recipe-system-setup timezone|locale|keymap|x11-keyboard|hostname|user|autologin|machine-id|host-keys|service ..." >&2; exit 2 ;;
esac
