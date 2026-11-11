# OS build recipes

This collection produces artifacts for the LevitateOS and AcornOS builder:
source root filesystems, configured installed systems, live overlays, kernels,
toolchains and tools. `sources/` holds package locks and `kernel/` holds shared
kernel facts. The builder selects this directory explicitly as its recipe
search path; variant declarations select the rootfs and preseed producers.

Every executable recipe uses the shared Recipe API 2 Rhai format and engine in
`tools/recipe`, with its own `NEEDS`. Its `install` hook writes an owned output
tree inside Ginux. Recipe publishes the declared files and directories, and the
builder consumes them to assemble images. recab owns activation and updates of
installed system images.

The desktop apps (`gui/` in the workspace) are compiled here too, but not in
Recipe's usual root: a Qt program has to be compiled against the distro's own
Qt and C++ runtime. The variant's `*-apps-build-root.rhai` recipe makes one
tree of the base lock and the apps-build lock (`sources/*-apps-build-lock.rhai`:
compiler, CMake, Qt's development files; on Fedora composed by the builder from
its unpack store, on Alpine installed by apk); the builder makes a root image
of it and runs `desktop-apps.rhai` in that root (`src/desktop_apps.rs`). The
build root is never part of an image.

A system tree is made in steps the builder caches separately
(`../README.md`, "The system tree"): `fedora-rpm-unpack.rhai` unpacks each
locked RPM into the builder's unpack store, the builder composes them on the
host, AcornOS's `alpine-apk-base-tree.rhai` installs its base lock with apk,
`*-package-effects.rhai` (over `package-effects.rhai`) applies what the
packages' scripts would have done, and the variant's rootfs recipe (over
`rootfs-producer.rhai`) applies `installed-system.rhai`. Trees cross from one
step to the next as one archive (`target_pack_tree`, `target_unpack_tree`).
`system-files.rhai` holds the file helpers the effects step shares with
`installed-system.rhai` without importing it.

Base and desktop packages, including applications bundled into a desktop image,
are image-owned here. The workspace's top-level
[`recipes/`](../../../recipes/README.md) is the separate catalog for software
users manage independently of the image. Both collections share one format and
engine; their consumers and output lifecycles differ. User package activation is
not implemented by these OS producers.
