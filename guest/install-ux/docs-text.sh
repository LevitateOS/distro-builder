#!/bin/sh
# install-docs: the install guide, wrapped to this terminal's width, in a pager.
guide=@GUIDE@
columns=$(stty size 2>/dev/null | cut -d' ' -f2)
[ "${columns:-0}" -gt 0 ] 2>/dev/null || columns=80
if command -v less >/dev/null 2>&1; then
    fold -s -w "$columns" "$guide" | LESS= less
else
    fold -s -w "$columns" "$guide"
fi
