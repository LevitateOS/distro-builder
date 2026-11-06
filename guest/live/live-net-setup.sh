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

# Retry because virtio NIC names can race (e.g. eth0 -> ens5), and keep
# applying until IPv4 + default route are stable.
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
            "$IP_BIN" addr replace 10.0.2.15/24 dev "$name" 2>/dev/null || true
            "$IP_BIN" route replace default via 10.0.2.2 dev "$name" 2>/dev/null || true
        fi
    done

    addr_if="$("$IP_BIN" -o -4 addr show 2>/dev/null | awk '$4 ~ /^10\.0\.2\.15\// {print $2; exit}')"
    default_if="$("$IP_BIN" -o -4 route show default 2>/dev/null | awk '/^default via 10\.0\.2\.2/ {for (i=1; i<=NF; i++) if ($i=="dev") {print $(i+1); exit}}')"
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
