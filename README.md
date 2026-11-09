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
- `recipes/`: the builder's Recipe scripts (source rootfs, installed system,
  live overlay, tools, kernel). Each recipe documents what it produces.
- `guest/`: files installed into the live and installed systems.
- `install-split-pane/`: the live install screen's two-pane terminal.

Ownership and output locations across the workspace are in the root
`architecture.md`.

## Recipe and product verification

The builder links the current `levitate-recipe` library and normally builds its
CLI from that same workspace source. `RECIPE_BIN` explicitly selects a prepared
CLI; select its matching `RECIPE_WORKER_BIN` too. The launcher variables forwarded
to Recipe are `GINUX_BIN`, `GINUX_ROOT`, `GINUX_ROOTFS`, `RECIPE_WORKER_BIN` and
`RECIPE_FRAME_TIMEOUT_SECS`. Other ambient `RECIPE_*`/`GINUX_*` controls are
removed. The maintained Ginux/Recipe source pair is recorded in
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
