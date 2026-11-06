#!/bin/sh
# @OS_NAME@ Live: Disable suspend actions
# Power button and lid close do nothing during live session

case "$1" in
    button/power)
        # Log but don't suspend - user is probably installing
        logger "@OS_NAME@ Live: Power button pressed (suspend disabled)"
        ;;
    button/lid)
        # Lid close does nothing - prevent accidental suspend
        logger "@OS_NAME@ Live: Lid event ignored (suspend disabled)"
        ;;
    *)
        # Let other events through to default handler
        ;;
esac
