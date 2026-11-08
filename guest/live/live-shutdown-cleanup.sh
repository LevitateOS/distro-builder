#!/bin/sh
# Live shutdown cleanup. The live root's read-only layers are EROFS images on
# loop devices, backed by files on the boot medium that recinit's live init
# keeps mounted at @LIVE_MEDIA_DIR@. Nothing can unmount them while the root
# uses them, so release them before the init unmounts filesystems: detach the
# medium lazily and detach every loop device that has a backing file (the
# kernel frees a device that is still in use when its last user goes).
status=0
fail() { echo "live-shutdown-cleanup: $*" >&2; status=1; }

media=@LIVE_MEDIA_DIR@
if mountpoint -q "$media"; then
    umount -l "$media" || fail "cannot detach $media"
fi

for loop in /sys/block/loop*; do
    [ -r "$loop/loop/backing_file" ] || continue
    device=/dev/${loop##*/}
    losetup -d "$device" || fail "cannot detach $device"
done

exit "$status"
