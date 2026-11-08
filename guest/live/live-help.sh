#!/bin/sh
cat <<'EOF2'
@OS_NAME@ Live Quick Help

Install commands:
  install-hub     the install steps in order
  disk-plan       plan, check and apply a disk layout
  base-system     copy the system onto the disk and write its fstab
  install-docs    the install guide on its own
  recsetup        set up the copied system: time zone, locale, accounts, ...
  recpart / recstrap / recfstab / recchroot   the steps by hand

On the install screen, its bottom line lists the keys. On a console that
runs the install screen, leaving the login shell (exit or Ctrl-D) brings
the screen back; the other consoles log root in to a shell.
EOF2
