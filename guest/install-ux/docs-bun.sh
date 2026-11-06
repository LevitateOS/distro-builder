#!/bin/sh
set -eu

APP="@DOCS_BUNDLE@"
if [ ! -r "$APP" ]; then
    echo "install docs bundle missing at $APP" >&2
    exit 1
fi

exec @BUN@ "$APP" "$@"
