# OS build recipes

This collection produces artifacts for the LevitateOS and AcornOS builder:
source root filesystems, configured installed systems, live overlays, kernels,
toolchains and tools. `sources/` holds package locks and `kernel/` holds shared
kernel facts. The builder selects this directory explicitly as its recipe
search path; variant declarations select the rootfs and preseed producers.

Every executable recipe uses the shared Recipe API 1 Rhai format and engine in
`tools/recipe` (`const RECIPE_API = 1;`), with its own `NEEDS`. Its `build(src, out)`
function writes the owned output (inside Ginux when it starts programs), or its
`SOURCES`, `FILES` and `PACKAGES` declarations do. Recipe publishes the declared
files and directories, and the builder consumes them to assemble images. A normal
recipe has one output; the kernel recipes publish one, `bundle`, holding the
kernel image and its modules, which the builder checks with `bundle_mismatch`
from `kernel/metadata.rhai`. A kernel built from source is three recipes,
each a normal step whose finished output Recipe keeps by its identity, so a
run interrupted after the hours-long compile does not compile again:
`linux-source` unpacks the pinned kernel.org source (or the `kernel_archive`
input), `linux-compile` configures it with the `kconfig` input and compiles
it, and `linux` installs the image and the modules into the bundle. The
previous bundle, given as `target` to `linux-compile` and to `linux`, is kept
when it was built from the same kconfig. `kernel/make.rhai` holds the `make`
environment the last two share. recab owns the two roots of an A/B
installation.

The builder runs every recipe here in Ginux. One that needs no compiler declares
`NEEDS.runtime: "minimal"` and runs in Recipe's minimal runtime image (a shell,
coreutils, findutils, sed, grep and what the Recipe worker needs, a few megabytes;
`tools/recipe/bootstrap/ginux-minimal-image.json`) instead of the 1.3 GB compiler
image: the preseeds, the unpack and base-tree steps, the APK package effects, the
rootfs recipes, both apps-build roots, the live overlay, the release medium,
`prebuilt-fetch`, `busybox-static`, `linux-prebuilt`, `linux-source`, `bun-toolchain`, `tui-binary`
and `root-effects`. What they start is a Recipe helper, a program of a root they
are given (through that root's loader), a downloaded static or glibc program
(`apk.static`, Bun) or those tools. The others stay in the compiler image for a
program only it has: `fedora-rpm-package-effects` (python3), `linux-deps`
(rpm2cpio, cpio), `linux-compile` and `linux` (make, gcc), `tool-crate` and `install-tools` (cargo,
gcc); `desktop-apps`, `xlibre` and `reclogin` run in the apps-build root.
A program added to a minimal recipe must be in that image's `keep` list, with the
libraries its `readelf -d` names.

The desktop apps (`tools/gui` in the workspace) are compiled here too, but not in
Recipe's usual root: a Qt program has to be compiled against the distro's own
Qt and C++ runtime. The variant's `*-apps-build-root.rhai` recipe makes one
tree of the base lock and the apps-build lock (`sources/*-apps-build-lock.rhai`:
compiler, CMake, Qt's development files; on Fedora composed by the builder from
its unpack store, on Alpine installed by apk); the builder makes a root image
of it and runs `desktop-apps.rhai` in that root (`src/desktop_apps.rs`); the
tree is published as a prebuilt, which the `rec-apps` group recipe pins. The
display server (`xlibre.rhai`) and the login screen (`reclogin.rhai`, which
links the distro's libpam, libinput and libxkbcommon and is compiled with the
distro's own Rust from a vendored offline source closure) are compiled in the
same root and published the same way (`src/display_server.rs`,
`src/login_screen.rs`). The build root is never part of an image.

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

Only the base system is image-owned here: no image carries software above it.
`root-effects.rhai` is the root effects recipe the base ships for Recipe, and
`release-medium.rhai` assembles the default software set's recipes and list
for the release medium and the package cache the builder cuts into the
release's category parts (the cache is not on the medium). The workspace's top-level
[`recipes/`](../../../recipes/README.md) is the separate catalog for software
users manage independently of the image. Both collections share one format and
engine; their consumers and output lifecycles differ. User package activation is
not implemented by these OS producers.
