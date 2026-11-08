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
  report are the variant's `preseed_outputs`.
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

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
