#!/bin/sh
# Live ISO shutdown cleanup:
# release loop-backed live mounts before systemd reaches umount.target.
set +e

for mp in /live-overlay /rootfs /run/live-media; do
    if mountpoint -q "$mp"; then
        umount "$mp" >/dev/null 2>&1 || umount -l "$mp" >/dev/null 2>&1
    fi
done

if [ -d /run/live-media ]; then
    rmdir /run/live-media 2>/dev/null || true
fi

for loopdev in /dev/loop1 /dev/loop0; do
    if [ -b "$loopdev" ]; then
        losetup -d "$loopdev" >/dev/null 2>&1 || true
    fi
done

if mountpoint -q /media/cdrom; then
    umount /media/cdrom >/dev/null 2>&1 || umount -l /media/cdrom >/dev/null 2>&1
fi

exit 0
