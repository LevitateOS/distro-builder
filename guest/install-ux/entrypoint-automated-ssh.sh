#!/bin/sh
set -eu

echo "[@DISTRO@] install experience: automated SSH"
echo "This ISO profile is intended for SSH-driven automation (qcow2/.img pipelines)."
exec "${SHELL:-@SHELL@}" -l
