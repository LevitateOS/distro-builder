# distro-builder

Builds a variant's products (rootfs, overlays, release ISOs) for LevitateOS and
AcornOS over Recipe. It reads each variant through `distro_contract::load`
(`distro/variants/<id>/distro.toml`); the product list is
`distro_contract::registry`.

## Where things live

- `src/parts.rs`: the cached parts a release is combined from, their keys,
  records and locks (see "Parts and the combiner" below).
- `src/release.rs`: the combiner, which writes a release ISO from its parts.
- `src/update_release.rs`: what makes an image updatable (the release's group
  recipes in the base, Recipe's `record-base`) and the update release a
  `release` build writes next to its ISO (see "Releases and updates" below).
- `src/pipeline/`: what goes into each part (the system tree, a product's
  overlay; `pipeline/products.rs`), the system sources' digest
  (`pipeline/sources.rs`), the kernel artifact layout (`pipeline/kernel.rs`)
  and Recipe's work directory (`pipeline/paths.rs`).
- `src/recipe/`: calling Recipe and reading what a recipe reports in
  the `outputs` of its result (`recipe/outputs.rs`). The outputs a rootfs-source preseed must
  report are read statically from that recipe's `NEEDS.outputs`. Recipes own
  network and resource grants; variant TOML selects recipes and binds inputs.
- `src/desktop_apps.rs`, `src/display_server.rs`: the Qt desktop apps from `tools/gui` and, for AcornOS, the display server compiled from source (see "Prebuilt programs").
- `src/artifact/`: EROFS and overlay images and filesystem copies.
- `distro_contract::runs` (`distro/contract`): the output directories (releases, parts) and the release run history.
- `src/bin/`: the `distro-builder` binary. Its `usage()` in
  `src/bin/distro-builder.rs` is the command reference. Normal use goes through
  `cargo xtask build` (`cargo xtask --help`).
- `os-recipes/`: OS artifact producers (source rootfs, installed system,
  live overlay, tools, kernel) and their shared modules and locks. Each recipe
  documents what it produces; [the collection guide](os-recipes/README.md)
  explains its boundary with user-managed software. The kernel recipes
  (`linux`, `linux-prebuilt`) publish one output, `bundle`; the builder checks
  the kernel image and the modules inside it (`bundle_mismatch` from
  `kernel/metadata.rhai`).
- `guest/`: shell files installed into the live and installed systems.
- `install-split-pane/`: the live install screen's two-pane terminal: a root
  shell on the left, the installation guide on the right.

## Two consumers of one Recipe engine

The builder selects `distro/builder/os-recipes` explicitly and consumes the
files and trees those recipes publish. Their `build` functions assemble outputs
(inside Ginux when they start programs); the builder turns those outputs into OS images. Every image is
the base system alone; software above the base (the desktop, its apps, audio,
network, printing...) is group recipes in `recipes/`, installed through Recipe
during the installation (docs/product-handoff.md 2.4). recab owns the two roots
of an A/B installation.

The workspace's top-level [`recipes/`](../../recipes/README.md) is the official
source of user-managed software, a repository of its own pinned here. The
Recipe tool package ships it as a source: `/usr/share/recipe/sources/levitate.toml`
says where it is fetched from, and `/usr/share/recipe/sources/levitate/` is a
copy at the pinned commit, with its published `index.json`, that Recipe uses
until it has fetched a newer index. It sits alongside the builder's selected
pinned Ginux root image and static launcher. Root's override catalog remains under `/var/lib/recipe`.

The builder always passes `--no-activate`: it consumes recipe outputs to compose
images, without activating packages on the build host. The user catalog is not
the builder's recipe search path.

Both collections use the same API 1 Rhai format and the same engine in
`tools/recipe`. The directory split identifies the consumer and lifecycle; it
does not introduce another recipe type or execution engine.

## Recipe and product verification

The builder links the current `levitate-recipe` library and normally builds its
CLI from that same workspace source (`RECIPE_BIN` explicitly selects a prepared
CLI). It builds Ginux's launcher from `tools/ginux` on first use in each run and
gives every invocation that launcher as `GINUX_BIN` and the CLI it runs as
`RECIPE_WORKER_BIN`; ambient values of those two are ignored (and logged), so a
build never runs an older launcher or worker than the checkout. `GINUX_ROOT`,
`GINUX_ROOTFS` and `RECIPE_FRAME_TIMEOUT_SECS` are forwarded; other ambient
`RECIPE_*`/`GINUX_*` controls are removed.

Each release records `system-sources.sha256` beside its rootfs: the digest of
every source its system rootfs is built from (the builder, contract, profile,
variant, the terminal apps in `tools/tui` and the Qt apps in `tools/gui`, the recipes repository and the rootfs tools' crate
closures, uncommitted edits included), the Ginux launcher built from
`tools/ginux` (by content: the system ships it as `usr/libexec/recipe/ginux`)
and Recipe's declared behaviour versions of the Recipe and Ginux that run the
recipe steps (`levitate_recipe::identity::declared`, not their bytes). It is
one input of the system part's key (below), so every
release carries a system built from the checkout's current sources. The
maintained Ginux/Recipe source pair is recorded in
`docs/ginux-contributions/delivery-sources.json` at the repository root. New local
consumer corrections still need integration and final-combination acceptance;
the contribution queue records their disposition.

The `tui-binary` recipe owns TUI dependency acquisition, compilation and checked
publication. Its separate `bun-toolchain` dependency acquires the pinned GNU
compiler and GNU/musl target runtime archives and verifies their hashes. Rust
supplies authored workspace files, the frozen root `bun.lock`, the selected app
entry and libc target as typed inputs. Host Bun and host `node_modules` are not
product inputs or product-build prerequisites. Host Bun is still used for
interactive TUI development and JavaScript checks.

The producer uses a frozen **hoisted** install with copyfile extraction, four
HTTP requests, an explicit 32,768-object storage request and hash-checked
readiness/reuse. Its npm authority is separate from the toolchain dependency's
GitHub authority. The configured runtime must provide the bounded owned FD-name,
stack metadata, eventfd/epoll and metadata-only O_PATH contracts documented by
Ginux. This compiler/installer path does not establish general Bun/JavaScript
runtime support inside Ginux. The installed programs run on the guest's Linux
kernel; their actual PTY behavior needs product/scenario verification.

## Parts and the combiner

A release ISO is combined from **parts**, each built once for what it is built
from and kept under `.artifacts/parts/<distro>/<kind>/<key>/`
(`docs/build-redesign.md`, section 0 at the repository root):

- `system`: the base system tree (`rootfs-source/`: the rootfs recipe's tree
  with the rootfs tools, the release's group recipes and the kernel's modules,
  its metadata completed, then Recipe's `base` entry from `record-base` as its
  last change), the rootfs EROFS, the installed initramfs and the release set
  (`release-set.json`, `live/` and `boot/` members, `system-sources.sha256`).
  Every product shares it.
- `overlay-<product>`: the product's overlay tree (live session with the
  `network` group, overlay tools, install screen; on the release medium also
  the recab app for A/B installs, the Updater for mutable installs and the
  default software set: its recipes, its
  list and the package cache of every file they pin, `src/default_set.rs`)
  built on the system part's tree, and its EROFS.

A part's key is a SHA-256 over its kind, the distro and named inputs, all
recorded in its `part.json` with its output files (path, bytes, sha256):

- system: `system-sources.sha256`'s digest (which covers the variant, so its
  release serial, and the release keys in `distro/keys/release`), the source closure of
  the builder's own crates (recinit, reciso, levitate-recipe...),
  Recipe's declared behaviour versions and the identity mode (`declared`, or
  `exact` with the Recipe CLI's and the Ginux launcher's SHA-256), the Ginux root (`GINUX_ROOT`, and the `GINUX_ROOTFS` tar's
  digest), the kernel (release, image digest, module tree digest) and busybox;
- overlay: the system part's key and exact build, the product, the overlay
  tools' crate closures and the running builder executable.

A part is built in a sibling temporary directory and renamed into place only
when complete. If its inputs changed while it was built (the checkout was
edited, a kernel installed), it is discarded and built again. A part under its
key is used only when its record names exactly today's inputs and every output
file has its recorded size and hash; otherwise it is removed and rebuilt. An
`flock` on each key lets two products or distros build at once; a second
builder of one key reuses the first one's part. The build log has one line per
part: `[part:system:levitate] reused <key prefix>`, `building ...` or
`built ...`; in exact mode the label ends in `:exact`.

**Identity modes.** By default the tools that execute recipe steps are named
by declared behaviour versions, in Recipe's saved-state identities and in the
system part's key, so a Recipe or Ginux rebuild that behaves the same reuses
every step whose inputs did not change (a Ginux change still builds the system
part again, because the system ships the launcher, but its Recipe steps are
reused). `release build <product> <distro> --exact` (`cargo xtask build
<product> <distro> --exact`) passes `--exact-identity` to every Recipe
invocation and puts the Recipe CLI's and launcher's bytes into the system
part's key, so a release comes only from what byte-identical tools built.
When to bump the declared versions is stated in
`tools/recipe/src/identity.rs`.

The **combiner** (`release build <product> <distro>`, `src/release.rs`)
ensures the product's system and overlay parts, hard-links their files into
the release run's directory (copies across filesystems; nothing is moved out of
a part), links `rootfs-source` and the overlay tree to the parts' trees for
readers such as the preflight, builds the live initramfs (never cached),
verifies the release set and writes the ISO with reciso. The run directory has
the same files as before parts existed. No product is built on another:
building test-install-base builds no test-base-rootfs or test-live-ssh ISO. `product prepare`
ensures the same parts and copies their trees into its directory, so
`transform build product-erofs` may change them there. Parts are never
removed automatically; delete `.artifacts/parts` to reclaim the space (old
releases' tree links then dangle).

## The system tree: unpack store, composition, effects, configuration

The system part's tree comes from `pipeline::source::materialize_rootfs_source`
in four steps, each cached by what determines it (`docs/build-redesign.md`,
sections 1 and 2; `src/compose.rs`), each a timeline step:

1. **Unpack store** (LevitateOS). `.artifacts/store/unpacked/<key>/` holds
   one locked RPM each: `root/` (what `target_extract_rpm` makes of it in an
   empty root, its target metadata manifest included), `members.json` (the
   record of that extraction: members in archive order, file capabilities, the
   directory mode extraction creates parents with) and `unpacked.json`. The key
   is a SHA-256 over the package's SHA-256, the sources of
   `os-recipes/fedora-rpm-unpack.rhai` (with its imports, by content),
   Recipe's `ENGINE_BEHAVIOUR`, the root image recipes run in and the store
   layout version: a Ginux or Recipe rebuild unpacks nothing. Missing packages
   are unpacked by `fedora-rpm-unpack.rhai` inside Ginux, split into shards of
   at most 512 MiB or 250 packages (at least one per worker), balanced by size;
   the CPUs minus one (at most 6) run at once, each its own Recipe invocation
   with its own work directory (`.artifacts/work/unpack/<pid>-<n>`), log
   (`.artifacts/work/unpack/logs/`) and progress file; their counts show as one
   in `cargo xtask progress`. A finished shard's trees move into the store at
   once (temp directory, then rename); entries are never deleted. Builds that
   share packages wait on `flock`s per key bucket (`.locks/<xx>.lock`, taken
   in order) and unpack only what is still missing.
2. **Composition** (LevitateOS, on the host, no Ginux, no package code).
   `.artifacts/store/composed/<key>/root/` is the packages merged in lock
   order by `levitate_recipe::target::replay`, which applies each package's
   recorded extraction exactly as extracting the packages one after another
   into one root does (same overwrite rules, same link resolution, same
   manifest); files are hardlinked from the unpack store. The key is a SHA-256
   over the compose format and the ordered unpack keys. The Fedora apps-build
   root is composed from the same store (base and apps-build locks, without firmware). Paths two
   packages both ship are listed in `composed.json` (path, both packages,
   whether identical) and counted in the log. `tree.tar`
   (`target::write_tree`'s format) is written on first use, and recipes get it
   with `packages.txt` as their `composed` directory input, which they check
   against their locks. AcornOS installs its base lock with one apk run
   instead (`alpine-apk-base-tree.rhai`, in
   `.artifacts/work/apk-base-tree/<distro>`, reused by Recipe's own state):
   apk's database is one run's, so APKs are not composed.
3. **Package effects** (`<backend>-package-effects.rhai` over
   `package-effects.rhai`): copies the composed tree (AcornOS: the base tree),
   applies
   what the packages' scripts would have done (on Fedora that includes the CA
   trust bundles `update-ca-trust` writes, which the builder generates), completes
   the target metadata and drops caches and logs. Its inputs are the composed tree and
   (AcornOS) the APKs: a configuration change never reruns it. On AcornOS it
   makes no BusyBox applet link for a command the desktop lock ships (its
   `commands()`), so a group recipe's full-size tool never meets a base file. It
   publishes the tree as one archive (`tree`).
4. **System configuration** (the variant's rootfs recipe over
   `rootfs-producer.rhai`): copies that archive's tree (input `system`),
   applies `installed-system.rhai` with the builder's configuration inputs
   (the base policy, the variant's branding, and the root effects recipe
   `os-recipes/root-effects.rhai` at `/usr/lib/recipe/effects.rhai`, which
   Recipe runs after each system change to rebuild the whole-root caches),
   completes the tree and runs the capability check. The desktop's shipped
   choices (Firefox's one bookmark, the Recipe launcher on the Plasma panel,
   the login screen, the look) are the group recipes' own configuration.

Acquire the base package closure explicitly with
`cargo xtask build rootfs-source <distro>` when it must be refreshed. A
release build and `product prepare` need the variant's kernel installed;
reuse the pinned prebuilt kernel (`cargo xtask build kernel <distro>
--prebuilt` when preparation is needed). Do not replace this with kernel
recompilation.

`cargo xtask build` runs this builder through `sudo -n` (passwordless sudo or root is required), the builder picks the compiler image pinned in `tools/recipe/bootstrap/product-ci.json` itself (an ambient `GINUX_ROOT`/`GINUX_ROOTFS` still wins) and a product build acquires its package locks itself; `artifact preseed-rootfs-source` remains for `--refresh`.

Configured Recipe needs whole-coordinator bootstrap privileges, and its private
publications need an appropriately privileged builder consumer. Configure the
launcher/image/worker explicitly as in Recipe's `docs/setup-linux.md`. A privileged
xtask build also needs the operator's explicitly selected installed Rust/Cargo
toolchain and dependency inputs; root's rustup defaults may be empty. Do not wrap
only the Ginux launcher in sudo. Producer commands build artifacts; scenarios
consume existing artifacts.

On enforcing SELinux hosts, prepare each root-built public ISO for read-only
session-libvirt access: `sudo chcon -t virt_content_t -- /absolute/path/to/product.iso`.
Mode 0644 alone does not make a `user_home_t` ISO readable by sVirt. This per-file
label preserves artifact bytes and checksums; private Recipe exports retain their
existing permissions. See `tools/recqemu/README.md` at the repository root.

After explicit builds, run `cargo xtask test all levitate` and
`cargo xtask test all acorn` in the operator's session-libvirt context.
The test CLI defaults to A/B installation (`--mode ab`). Selecting `--mode mutable`
skips `other-root` in `all`; a standalone `other-root` still needs an A/B install. The
`test-install-base` scenario retains actual guest installer PTY interaction in
`install-pty.log`, in addition to its tool/split-pane smoke controls. Version/help
or smoke alone does not establish interaction. These commands are acceptance
procedures, not claims that a particular source/product combination passed.
The contribution queue owns current results and final delivery dependencies.

## Where the tools go

`src/pipeline/live_tools.rs` (`LIVE_TOOLS`) says which tool goes where:

- On the live medium only (the product's overlay): `recpart`, `recstrap`,
  `recfstab`, `recchroot`, `recsetup`, `recab` and `install-split`. `recab`
  is never in the system image. `recsetup`'s A/B boot step puts it, its
  `recab-started` service and, from the release medium, the recab app files
  (`AB_DESKTOP_APP_FILES`, carried under `/usr/lib/recsetup/ab-only`) into the
  target. A mutable install gets none of them.
- On the live medium, for a mutable install only: the Updater app
  (`MUTABLE_DESKTOP_APP_FILES`, carried under `/usr/lib/recsetup/mutable-only`),
  which `recsetup`'s mutable boot step copies into the target. An A/B install
  never gets it.
- In the system image: `recipe`, `recguest` and `recupdate`. Every system
  starts `recguest` only when the machine has its virtio port. `recupdate`'s
  tree is made per init system (`install-tools.rhai`, `INIT`): on systemd its
  `recupdate-notice.service` and `.timer` (enabled), on OpenRC
  `/etc/periodic/daily/recupdate-notice` (BusyBox's `crond`, enabled by
  `installed-system.rhai`, runs it), on both the autostart entry
  `/etc/xdg/autostart/recupdate-notify.desktop`, and the release public keys
  (`distro/keys/release/*.pub`) at `/usr/share/levitate/release-keys/`. A
  build without a key says so in one line; such an image refuses every
  release. These are on the live system too (it runs the system image).

## Timeline and progress

Every line the builder prints starts with the local time (`HH:MM:SS`; `tprintln!`/`teprintln!` in `src/timeline.rs`). `release build` records its run with `timeline::start`/`finish` (`src/bin/workflows/build.rs`); each part ensure (`reused` or `built`, with its short key), each combiner and part step (reading the kernel, EROFS writes, both initramfs builds, the ISO write), each Recipe invocation (by recipe file name) and each tool compile (`cargo build` of Recipe and Ginux) is a step that nests in the one around it.

- `<run directory>/timeline.jsonl`: two lines per step, `{"event":"start"|"end","step","kind","key"?,"note"?,"depth","at", ...}`; an `end` also has `start`, `seconds` and `ok`. Appended as it happens, so a killed build keeps what it did.
- `<run directory>/timeline.txt`, also printed at the end (and on failure): "where the time went", each step with its duration and share of the build.
- `.artifacts/progress/<distro>.json`: the current state (`distro`, `product`, `run_dir`, `pid`, `state` = `running`/`finished`/`failed`, `started`, `updated`, the step stack, the active Recipe invocation), replaced atomically at every change.
- `.artifacts/progress/<distro>/<recipe>-<pid>.json`: Recipe's own progress file. `Invocation::run` passes it as `--progress-file` to every `install` invocation and removes it when the invocation succeeds. Recipes report through `progress(done, total, what)`; `total` 0 means "no count". Recipe writes it at most once a second.

`cargo xtask progress` reads these (see `xtask/README.md`). Nothing here can fail a build: an unwritable file is skipped.

## Prebuilt programs

Our own tools (`recpart`, `recstrap`, `recab`, `recguest`, `recupdate`, `recipe`, `install-split`), the TUI
binary (`install-tui`), the Qt desktop apps (family `desktop-apps`, one whole tree per distro: programs, menu entries, polkit actions, Recipe's metadata, kept with exact modes and links) the display server (family `display-server`, AcornOS only) and the login screen (family `login-screen`, reclogin, one tree per distro) are identified by a **key**, the SHA-256 of
everything their compile depends on (`src/prebuilt.rs`): the prepared source closure,
the compile recipe, the target, the compiler image digest and Recipe's two behaviour
versions (`ENGINE_BEHAVIOUR`, `GINUX_BEHAVIOUR`; `prebuilt::behaviour`). Nothing that does
not change what the compile writes is in it: not Ginux's contract digests, and not a
lock's `locked_on` date. So CI builds and publishes a part only when the program would
differ. A build takes each program from the
first of: the local cache `.artifacts/prebuilt/<family>/<key>/` (every file checked
against `prebuilt.json`), a download of release `<family>-<key16>` from `LevitateOS/prebuilt`
(`LEVITATE_PREBUILT_REPO` overrides; fetched by `os-recipes/prebuilt-fetch.rhai`, verified
again), or a compile, which fills the cache. Any download failure is a miss. One line per
program says which: `[prebuilt] recab: local|downloaded|compiled <key16>`.

- `release build --exact` never uses prebuilts and touches no cache.
- `--no-prebuilt` (`cargo xtask build ... --no-prebuilt`) compiles but still fills the cache.
- `distro-builder prebuilt key <tools|tui|desktop-apps|display-server|login-screen> <name> [--distro <id>]` prints the key (CI uses it);
  a TUI key needs `--distro` for its libc; for `desktop-apps`, `display-server` and `login-screen` the name is the distro and no flag is taken.
- The desktop apps key covers `tools/gui` (tracked files, modes, worktree content), the base and apps-build
  locks, the apps-build root, both preseed recipes and `desktop-apps.rhai` with their imports, the
  compiler image, Recipe's declared versions, the distro and its libc. On a hit `desktop_apps.rs` skips
  building the apps-build root and the compile; the cached tree is merged and `check` still runs.
  `prebuilt build desktop-apps <distro>` (elevated) fills the cache without ISO or download.
- The login screen (`src/login_screen.rs`, `os-recipes/reclogin.rhai`) links the distro's own libpam,
  libinput and libxkbcommon, so it is compiled like the desktop apps: in the apps-build root, with
  the distro's own Rust (`rust`, `cargo` and the libraries' development packages are in
  `desktop.apps_build_packages`). Its source is the offline closure the static tools use
  (`recipe::tools::prepare_crate_source`: the crate with `dist/` and `assets/`, a one-member
  workspace inheriting the root's package fields, the pruned `Cargo.lock`, every crate vendored),
  compiled `--locked --offline` with no network. Its key covers that closure, `reclogin.rhai`, the
  distro's `os_id` and init system, the apps-build root (both locks, its recipes), the compiler
  image, Recipe's declared versions, the distro and its libc. `prebuilt build login-screen <distro>`
  (elevated) fills the cache; no image carries the tree: the `login-screen` group recipe pins it
  (prepared in `tools/reclogin/dist/recipe/`, not yet in `recipes/`).
- `distro-builder prebuilt publish <tools|tui|desktop-apps|display-server|login-screen> <name> [--distro <id>]` (also `cargo xtask
  prebuilt publish ...`, run as the user) uploads the cache entry of the current key with
  `gh`: it creates the release if missing, never replaces an asset with different bytes, and
  uploads `<family>-<name>.tar.gz` before `prebuilt.json`.
- `distro-builder prebuilt publish-runtime` (also `cargo xtask prebuilt publish-runtime`, run as the
  user) compresses the Recipe runtime image the installed Recipe ships (`zstd -T0 -19`) and uploads
  it as `rootfs.tar.zst`, with `runtime.json`, to release `recipe-runtime-<digest16>`; a release that
  already has `rootfs.tar.zst` is left alone. Recipe downloads it on first use.

## Release set

The system part writes `release-set.json` (`distro_contract::release`) beside
its other outputs. It is a small record of what the build produced: the
system's identity, the digest of its sources, its kernel release and
module-tree digest, and the size and SHA-256 of the rootfs image, kernel and
installed initramfs under their ISO paths (`live/`, `boot/`). The combiner
checks that set again in each release directory (identity, every member's
size and hash, the module tree of the system tree it was built from) and puts
it on the ISO. Preflight checks a release run against it. The format is in
[the release contract](../contract/docs/releases.md).

## Releases and updates

docs/release-engineering.md (at the repository root) is the design; the
variant's `[identity]` names the release: `os_version` (`1.0`, ...),
`release_serial` (raised by hand for every release, never automatically) and
`release_notes` (an https URL).

Every image is updatable (`src/update_release.rs`): the system tree carries
the release's group recipes (every `recipes/*.rhai` of `ABOUT.kind` `system`
or `library` that names the OS in `ABOUT.systems`) at
`/usr/lib/recipe/release-recipes/`, and its last change is Recipe's
`record-base` (the receipt entry `base` and `/usr/lib/levitate/release.json`
with the release's `schema`, `os`, `version`, `serial`, `date` and `notes`;
it cannot carry the digests of archives made from the tree it is in).

`release build release <distro>` (`cargo xtask build release <distro>`) then
writes the update release in the run directory, next to the ISO:

- `base.tar.zst`: the system tree, by the same tar writer as the EROFS image
  (`levitate_recipe::target::write_tar`: target metadata, numeric owners, a
  member per directory, pax xattrs, sorted names, mtime 0), compressed with
  `zstd -T0 -19`, in the format Recipe's `src/release.rs` documents;
- `boot.tar.zst`: `vmlinuz` and `initramfs.img` (the installed initramfs);
- `recipes.tar.zst`: the release's group recipes, flat;
- `release.json`: the unsigned manifest (schema 1; `url`, `sha256`, `size`
  empty, `recipes.revision` the `recipes` submodule's commit).

`cargo xtask release publish <distro>` signs and publishes that folder. The
test products' run directories have no update release.

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
