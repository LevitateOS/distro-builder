# distro-builder

Builds a variant's products (rootfs, overlays, release ISOs) for LevitateOS and
AcornOS over Recipe. It reads each variant through `distro_contract::load`
(`distro/variants/<id>/distro.toml`); the product list is
`distro_contract::registry`.

## Where things live

- `src/pipeline/`: planning and preparing each product, the kernel artifact
  layout (`pipeline/kernel.rs`) and Recipe's work directory (`pipeline/paths.rs`).
- `src/recipe/`: calling Recipe and reading what a recipe reports in
  `ctx.outputs` (`recipe/outputs.rs`). The outputs a rootfs-source preseed must
  report are read statically from that recipe's `NEEDS.outputs`. Recipes own
  network and resource grants; variant TOML selects recipes and binds inputs.
- `src/artifact/`: EROFS and overlay images and filesystem copies.
- `src/run_history.rs`: the output directories and release run history.
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
variant, TUI apps and the rootfs tools' crate closures, uncommitted edits
included) and the Ginux launcher built from `tools/ginux`. A release build first rebuilds every parent product that is missing
or whose recorded digest differs from the checkout's, and preparing a product
refuses such a parent, so live-boot and live-tools always carry the current
system and tools. The maintained Ginux/Recipe source pair is recorded in
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

Acquire the base package closure explicitly with
`cargo xtask build rootfs-source <distro>`. Then build `base-rootfs`, `live-boot`
and `live-tools` in that order for a complete fresh chain. A derived product uses
the latest successful parent; it does not automatically rebuild every existing
parent when Recipe, inputs or tooling change. Reuse the pinned prebuilt kernel
(`cargo xtask build kernel <distro> --prebuilt` when preparation is needed).
Do not replace this with kernel recompilation or a desktop build.

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

Release assembly emits `release.json`, `system-inventory.json` and, when
configured, `release.sig` beside its existing outputs. The ISO carries the exact
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

To configure online acquisition, set the variant's source and trust deliberately:

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

For production, the server must expose that directory's layout at the exact
configured HTTPS index URL. Deploy immutable `releases/` objects and
`index-signatures/` before replacing `index.json`, and retain older referenced
objects. Copying only `index.sig` is insufficient: recab requests
`index-signatures/<SHA256(index.json)>.sig`. Keep the index refreshable by clients
and serve the signed bytes without rewriting them.

P14 remains unconfigured: both distro contracts still have development sequence
1 with no production origin or trust key. The real index URL/channel, public
release key, external signing-key custody and deployment destination/access
must be supplied before configuration or publication. Existing local fixture
passes do not establish a production service. Installed check/download/apply
against the published service and applicable current runtime evidence remain
outstanding; no full build or VM run is implied by the publishing commands.

The format and trust rules are in
[the shared release contract](../contract/docs/releases.md). Full new distro,
ISO and kernel builds have not been run for P07/P08.
