#!/bin/sh
set -eu

IP_BIN="$(command -v ip 2>/dev/null || true)"
if [ -z "$IP_BIN" ]; then
    if [ -x /usr/sbin/ip ]; then
        IP_BIN=/usr/sbin/ip
    elif [ -x /usr/bin/ip ]; then
        IP_BIN=/usr/bin/ip
    else
        exit 1
    fi
fi

# Let NetworkManager acquire the network offered by the boot environment.
# A libvirt passt guest can use the host's subnet, so QEMU's historical
# 10.0.2.15/24 and 10.0.2.2 defaults are not portable.
NMCLI_BIN="$(command -v nmcli)"

# Retry because virtio NIC names and NetworkManager startup can race.
stable_ok=0
i=0
while [ "$i" -lt 45 ]; do
    for dev in /sys/class/net/*; do
        [ -e "$dev" ] || continue
        name="$(basename "$dev")"
        case "$name" in
            lo|sit*|ip6tnl*|tun*|tap*|dummy*|bond*|veth*|docker*|virbr*|br*|wg*)
                continue
                ;;
        esac
        if "$IP_BIN" link set "$name" up 2>/dev/null; then
            if ! "$IP_BIN" -4 route show default dev "$name" | grep -q '^default '; then
                "$NMCLI_BIN" --wait 5 device connect "$name" >/dev/null 2>&1 || true
            fi
        fi
    done

    default_if="$("$IP_BIN" -o -4 route show default 2>/dev/null | awk '/^default / {for (i=1; i<=NF; i++) if ($i=="dev") {print $(i+1); exit}}')"
    addr_if=""
    if [ -n "$default_if" ]; then
        addr_if="$("$IP_BIN" -o -4 addr show dev "$default_if" scope global 2>/dev/null | awk 'NR == 1 {print $2}')"
    fi
    if [ -n "$addr_if" ] && [ "$addr_if" = "$default_if" ]; then
        stable_ok=$((stable_ok + 1))
        if [ "$stable_ok" -ge 3 ]; then
            exit 0
        fi
    else
        stable_ok=0
    fi

    i=$((i + 1))
    sleep 1
done

exit 1
