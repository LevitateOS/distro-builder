# distro-builder

Builds a variant's products (rootfs, overlays, release ISOs) for LevitateOS and
AcornOS over Recipe. It reads each variant through `distro_contract::load`
(`distro/variants/<id>/distro.toml`); the product list is
`distro_contract::registry`.

## Where things live

- `src/parts.rs`: the cached parts a release is combined from, their keys,
  records and locks (see "Parts and the combiner" below).
- `src/release.rs`: the combiner, which writes a release ISO from its parts.
- `src/pipeline/`: what goes into each part (the system tree, a product's
  overlay; `pipeline/products.rs`), the system sources' digest
  (`pipeline/sources.rs`), the kernel artifact layout (`pipeline/kernel.rs`)
  and Recipe's work directory (`pipeline/paths.rs`).
- `src/recipe/`: calling Recipe and reading what a recipe reports in
  `ctx.outputs` (`recipe/outputs.rs`). The outputs a rootfs-source preseed must
  report are read statically from that recipe's `NEEDS.outputs`. Recipes own
  network and resource grants; variant TOML selects recipes and binds inputs.
- `src/artifact/`: EROFS and overlay images and filesystem copies.
- `distro_contract::runs` (`distro/contract`): the output directories (releases, parts) and the release run history.
- `src/bin/`: the `distro-builder` binary. Its `usage()` in
  `src/bin/distro-builder.rs` is the command reference. Normal use goes through
  `cargo xtask build` (`cargo xtask --help`).
- `os-recipes/`: OS artifact producers (source rootfs, installed system,
  live overlay, tools, kernel) and their shared modules and locks. Each recipe
  documents what it produces; [the collection guide](os-recipes/README.md)
  explains its boundary with user-managed software.
- `guest/`: files installed into the live and installed systems.
- `install-split-pane/`: the live install screen's two-pane terminal.

Ownership and output locations across the workspace are in the root
`architecture.md`.

## Two consumers of one Recipe engine

The builder selects `distro/builder/os-recipes` explicitly and consumes the
files and trees those recipes publish. Their `install` hooks assemble outputs
inside Ginux; the builder turns those outputs into OS images. Packages included
in a base or desktop image, including desktop applications, belong to that
image's lifecycle. recab owns staging and switching installed system images.

The workspace's top-level [`recipes/`](../../recipes/README.md) is the official
source of user-managed software, a repository of its own pinned here. The
Recipe tool package ships it as a source: `/usr/share/recipe/sources/levitate.toml`
says where it is fetched from, and `/usr/share/recipe/sources/levitate/` is a
copy at the pinned commit, with its published `index.json`, that Recipe uses
until it has fetched a newer index. It sits alongside the builder's selected
pinned Ginux root image and static launcher. Root's override catalog remains under `/var/lib/recipe`. On immutable A/B
installs, Recipe package commands target recab's inactive preparation, and system
software is selected under the booted slot's `/usr/lib/recipe/profile`. Login
shells add that profile and ordinary users' independent profiles to `PATH`.

The builder always passes `--no-activate`: it consumes recipe outputs to compose
images, without activating packages on the build host. The user catalog is not
the builder's recipe search path.

Both collections use the same API 2 Rhai format and the same engine in
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
variant, TUI and GUI apps, the recipes repository and the rootfs tools' crate
closures, uncommitted edits included) and the Ginux launcher built from
`tools/ginux`. It is one input of the system part's key (below), so every
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

- `system-base` and `system-desktop`: the system tree (`rootfs-source/`: the
  rootfs recipe's tree with the rootfs tools and apps, the kernel's modules
  and the kernel binding `usr/lib/recab/kernel.json`, metadata finalized), the
  rootfs EROFS, the installed initramfs and the release set (`release.json`,
  `system-inventory.json`, signature, `live/` and `boot/` members,
  `system-sources.sha256`). base-rootfs, live-boot, live-tools and
  installed-boot share `system-base`; live-desktop has `system-desktop`.
- `overlay-<product>`: the product's overlay tree (live session, overlay
  tools, install screen) built on its system part's tree, and its EROFS.

A part's key is a SHA-256 over its kind, the distro and named inputs, all
recorded in its `part.json` with its output files (path, bytes, sha256):

- system: the flavour, `system-sources.sha256`'s digest, the source closure of
  the builder's own crates (recinit, reciso, recab, levitate-recipe...), the
  Recipe CLI, the Ginux root (`GINUX_ROOT`, and the `GINUX_ROOTFS` tar's
  digest), the kernel (release, image digest, module tree digest), busybox,
  the release policy and its production authority, and whether the manifest
  is signed and the digest of the `RECAB_RELEASE_SIGNING_KEY` file;
- overlay: the system part's key and exact build, the product, the overlay
  tools' crate closures and the running builder executable.

A part is built in a sibling temporary directory and renamed into place only
when complete. If its inputs changed while it was built (the checkout was
edited, a kernel installed), it is discarded and built again. A part under its
key is used only when its record names exactly today's inputs and every output
file has its recorded size and hash; otherwise it is removed and rebuilt. An
`flock` on each key lets two products or distros build at once; a second
builder of one key reuses the first one's part. The build log has one line per
part: `[part:system-desktop:levitate] reused <key prefix>`, `building ...` or
`built ...`.

The **combiner** (`release build <product> <distro>`, `src/release.rs`)
ensures the product's system and overlay parts, hard-links their files into
the release run's directory (copies across filesystems; nothing is moved out of
a part), links `rootfs-source` and the overlay tree to the parts' trees for
readers such as the preflight, builds the live initramfs (never cached),
verifies the release set and writes the ISO with reciso. The run directory has
the same files as before parts existed. No product is built on another:
building live-tools builds no base-rootfs or live-boot ISO. `product prepare`
ensures the same parts and copies their trees into its directory, so
`transform build product-erofs` may change them there. Parts are never
removed automatically; delete `.artifacts/parts` to reclaim the space (old
releases' tree links then dangle).

Acquire the base package closure explicitly with
`cargo xtask build rootfs-source <distro>` when it must be refreshed. A
release build and `product prepare` need the variant's kernel installed;
reuse the pinned prebuilt kernel (`cargo xtask build kernel <distro>
--prebuilt` when preparation is needed). Do not replace this with kernel
recompilation or a desktop build.

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
`cargo xtask test all acorn` in the operator's session-libvirt context, then
`cargo xtask test ab-update levitate` and `cargo xtask test ab-update acorn`.
The current test CLI defaults to A/B installation. Selecting `--mode mutable`
skips A/B in `all`; a standalone `ab-update` still needs an A/B install. The
`live-tools` scenario retains actual guest installer PTY interaction in
`install-pty.log`, in addition to its tool/split-pane smoke controls. Version/help
or smoke alone does not establish interaction. These commands are acceptance
procedures, not claims that a particular source/product combination passed.
The contribution queue owns current results and final delivery dependencies.

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Release manifest

The system part emits `release.json`, `system-inventory.json` and, when
configured, `release.sig` beside its other outputs, and the combiner verifies
that set again in each release directory before writing the ISO. The ISO carries the exact
same manifest/inventory/signature as the online bundle; payload members use its
`boot/` and `live/` layout. The image writer records intended metadata from the
actual tar input and binds it to the resulting EROFS image. Release emission
refuses missing/stale image provenance, a changed prepared tree or mismatched
kernel, modules, initramfs, identity or runtime protocol.

To emit/verify metadata for **already produced** artifacts, without starting a
system build:

```sh
distro-builder --repo-root /path/to/LevitateOS release manifest \
  levitate base /path/to/prepared/rootfs-source /path/to/release
```

The release directory must contain the rootfs image, its image-writer provenance,
`system-sources.sha256`, `boot/vmlinuz` and `boot/initramfs-installed.img`. The
prepared tree must still match the image. An older image without provenance is
not silently blessed by supplying matching filenames.

Configure release ordering and optional source/trust in the variant's `[release]`
contract. The checked-in development channels have no production endpoint/key.
Signed emission requires `--signing-key /private/release.pk8`; full release
assembly reads that explicit path from `RECAB_RELEASE_SIGNING_KEY`. The key must
match the configured distribution public key. Key custody and deployment of the
published directory remain release operations. Manifest and index publication
refuse signing keys inside the checkout, prepared root, release bundle or
publication directory (using resolved paths). Key format and authority are
checked before public release objects are written. Supply an external private
key file; do not put key bytes in command arguments, defines or logs.

The approved P14 production design starts with separate root, release and index
roles and an explicit version-2 policy; it does not reinterpret the legacy
`release.trust` key. These preparatory commands use configured public production
policy and disposable or externally held private keys:

```sh
distro-builder --repo-root /path/to/LevitateOS release authorize \
  levitate /path/to/public-role-document.json /path/to/feed/release-keys.json \
  --root-key /private/root.pk8
distro-builder --repo-root /path/to/LevitateOS release verify-feed \
  levitate /path/to/feed
```

Authorization signs exact public document bytes and verifies role separation,
scope, validity and monotonic replacement. It can renew expired, correctly
root-signed authorization. Feed verification checks the root proof, index-role
signature, audience/epoch/freshness, release-role signatures and transport maps
against the existing system manifests. Its JSON explicitly reports that payloads
were not fetched/verified and production was not enabled. These commands do not
build, upload or change an installed system. Private key files must also exclude
group/other access. The delegated reader, transport publisher and publication/refresh
workflows are implemented. Public-origin installed scenario support and applicable
runtime validation remain pending; existing variant defaults remain disabled.
See `tools/recab/RELEASE-ENGINEERING.md` for the approved design.

For legacy development feeds, configure the single-key source explicitly:

```toml
[release.source]
index = "https://updates.example.org/development/index.json"
recipe = "distro/builder/os-recipes/update-acquire.rhai"
[release.trust]
ed25519 = "<64 lowercase hex characters of the public release key>"
```

The example URL is a placeholder, not a configured LevitateOS service. The builder
packages the private acquisition recipe with only that endpoint in `NEEDS.network`
and writes its installed path into the root policy. It must contain exactly one
`network: [] /* recab configured endpoint */` marker. Production signing keys
stay outside the checkout and published directory.

Publish an already signed bundle into a local serveable directory:

```sh
distro-builder --repo-root /path/to/LevitateOS release index \
  levitate /path/to/release /path/to/published 1 --signing-key /private/release.pk8
```

The index sequence must advance. The publisher serializes writers, copies and
checks immutable `releases/<manifest SHA256>/` objects, signs the index, publishes
its content-addressed signature first and replaces `index.json` last. Retain
older release objects/signatures for clients with pinned downloads. Serving or
uploading that directory is explicit; this command performs no network publish.

For this legacy feed, the server exposes that directory's layout at the exact
configured HTTPS index URL. Deploy immutable `releases/` objects and
`index-signatures/` before replacing `index.json`, and retain older referenced
objects. Copying only `index.sig` is insufficient: recab requests
`index-signatures/<SHA256(index.json)>.sig`. Keep the index refreshable by clients
and serve the signed bytes without rewriting them.

Production enablement remains pending: both distro contracts still have development
sequence 1 with no production origin or trust key. The approved Pages/immutable
Release service and separate signing roles are implemented, but canonical origin
confirmation, public authority, private-key custody, protected environments and
release runtime provisioning still need to be completed. Existing local fixture
passes do not establish a production service. Installed check/download/apply
against the published service and applicable current runtime evidence remain
outstanding; no full build or VM run is implied by the publishing commands.

The format and trust rules are in
[the shared release contract](../contract/docs/releases.md). Full new distro,
ISO and kernel builds have not been run for P07/P08.


Production x86_64 transport and metadata publication use `release transport`,
`release publish` and `release verify-online`. `cargo xtask release` supplies the
operator upload/refresh/deploy workflow; see the superproject's
[production publication guide](https://github.com/LevitateOS/LevitateOS/blob/master/release/updates/README.md).
These consume existing signed artifacts and explicit Recipe runtime inputs.
They do not build systems or establish installed production readiness. The
legacy `release index` command remains for development feeds and refuses a
production policy. Production origins, keys and stable enablement remain
pending provisioning and applicable validation.
