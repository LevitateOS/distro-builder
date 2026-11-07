# distro-builder

Shared infrastructure for building Linux distribution ISOs.

This crate provides common abstractions used by both [leviso](https://github.com/LevitateOS/leviso) (LevitateOS) and [AcornOS](https://github.com/LevitateOS/AcornOS) builders.

## Architecture

```
distro-builder (this crate)
    │
    ├── component/     Declarative component system
    │   └── Installable trait, Phase enum, generic Op variants
    │
    ├── build/         Build utilities
    │   ├── context    DistroConfig and BuildContext traits
    │   └── filesystem FHS directory structure utilities
    │
    ├── artifact/      Artifact builder interfaces
    │   ├── rootfs     mkfs.erofs wrapper (EROFS only)
    │   ├── initramfs  cpio+gzip builder
    │   └── iso        xorriso wrapper
    │
    └── preflight/     Host tool validation
```

## Usage

```rust
use distro_builder::component::{Installable, Op, Phase};
use distro_builder::build::context::DistroConfig;

// Implement DistroConfig for your distribution
struct MyDistroConfig;

impl DistroConfig for MyDistroConfig {
    fn os_name(&self) -> &str { "MyDistro" }
    fn os_id(&self) -> &str { "mydistro" }
    // ... other methods
}

// Define components using the Installable trait
struct MyComponent;

impl Installable for MyComponent {
    fn name(&self) -> &str { "MyComponent" }
    fn phase(&self) -> Phase { Phase::Services }
    fn ops(&self) -> Vec<Op> {
        vec![
            Op::Dir("etc/myservice".into()),
            Op::WriteFile("etc/myservice/config".into(), "key=value".into()),
        ]
    }
}
```

## Build model

Builds read the variant contract `distro/variants/<distro>/distro.toml` through `distro_contract::load`: sources, filesystem products, artifact transforms, release and scenarios, with the kernel configuration (`kconfig`) and release hooks (`hooks/`) beside it. The kernel artifact layout (`kernel-build/include/config/kernel.release`, `staging/boot/vmlinuz`, `staging/usr/lib/modules`) and the kernel recipe are constants in `distro_builder::kernel`.

Build explicitly with `cargo xtask build live-tools levitate`. Run an independent scenario with `cargo xtask test live-tools levitate`; scenario commands consume existing artifacts. The workspace `architecture.md` describes product outputs, scenarios, and validation.

## Related Crates

- [distro-contract](../contract) - Variant contracts (the per-distro declarations)
- [recinit](../../tools/recinit) - Live initramfs and the live-boot protocol
- [reciso](../../tools/reciso) - ISO assembly

## Migration Notes

- [docs/00_MIGRATION_INDEX.md](docs/00_MIGRATION_INDEX.md) - migration index for numbered distro-builder migration tracks
- [docs/01_MIGRATION_FEDORA_SWAP.md](docs/01_MIGRATION_FEDORA_SWAP.md) - active Fedora Server DVD swap plan for the LevitateOS Stage 01 source path
- [docs/03_MIGRATION_STAGELESS.md](docs/03_MIGRATION_STAGELESS.md) - historical plan for the completed transition to product-oriented ownership

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Recipe output contracts

Producer recipes report named path products in `ctx.outputs`. Readiness checks
must return those names when reusing an existing installation. The Rust adapters
require explicit absolute paths and validate the selected filesystem types:

| Consumer | Required named outputs |
| --- | --- |
| Rootfs source | `rootfs` directory |
| Fedora ISO preseed | `iso` file |
| Alpine asset preseed | `iso` and `apk_tools` files |
| Kernel | `kernel` file and `modules` directory; optional `source` directory |
| Legacy Alpine facade | `iso` file and `rootfs` directory |

Kernel metadata also requires a nonempty `ctx.kernel_version`; there is no
`unknown` fallback. `LinuxPaths.source` is optional because installed kernel
artifacts can be reused without retaining source caches. Legacy flat fields may
remain in recipe context during migration, but adapters do not fall back to them
or to guessed filenames/directories.

Each CLI invocation uses a fresh temporary JSON result path. Missing or malformed
results fail instead of reusing an old `.recipe-output.json`.

These are consumer contracts, not publication authority. The Ginux coordinator
must export selected domain products and supply published host paths before
these adapters consume them. That R08 integration and real managed runtime
verification remain pending; a declared domain path is not publication evidence.

The integration target `recipe_outputs` invokes the real Recipe CLI against a
self-contained rootfs producer and executes the returned tool. It also checks
missing/wrong-kind/unproduced outputs with a stale conventional rootfs present.
Build `recipe` explicitly and configure a real `GINUX_BIN` before running it.

The `real_cli_success_without_a_fresh_json_result_cannot_reuse_stale_context`
case uses the actual static `recipe info` command to verify that exit zero
without a fresh JSON result cannot reuse cached context. That case does not
evaluate recipe code and can run without Ginux; it does not validate publication.

## Recipe work paths

`recipe::recipe_work_dir(repo_root, distro_id)` selects
`.artifacts/work/<distro>/downloads` for shared acquisition/build work. Canonical
kernel checks, xtask source/prebuilt producers, and explicit rootfs preseeding
use this path owner. Published kernel artifacts stay under
`.artifacts/kernel/<distro>/current`.

Use `recipe::linux::linux_for_distro(repo_root, distro_id)` for the source kernel
producer. It reads configuration from the canonical variant contract rather than
searching the legacy distro crate. The older `linux(base_dir, ...)` entry point
remains a compatibility wrapper. `has_linux_source` remains a filesystem cache
hint for compatibility; it does not establish readiness or the validity of an
installed kernel. Legacy Alpine/package/tool adapters still need path migration.

## Installed system and live environment

The Fedora/Alpine source recipes publish a complete installed filesystem. Both
variants consume that whole tree; Ring 2 does not maintain a second list of
package-owned paths. `installed-system.rhai` prepares authenticated consoles,
network defaults, account policy and the target setup program, importing the
variant's declared `system_setup` script input. Reusable images have a locked
root account and an empty machine ID.

`live-overlay.rhai` separately produces the live session's login overrides,
password override, service links, network helpers and shell profiles. The Rust
adapter supplies explicit configuration/assets and consumes `outputs.overlay`.
It preserves the source tree's system accounts when overriding root for the
live session. The product consumer refuses stale parent trees with live settings
in their installed filesystem.

Workspace tool recipes publish `outputs.program` and `outputs.rootfs`: the latter
is the installed package tree, including recab's init-system-specific trial
service. Product assembly selects its destination layer and merges that tree.
`recsetup` invokes the target program installed by Recipe for machine choices;
`recstrap` remains an image extractor. Scenario evidence scripts are transferred
by the harness into `/run`, rather than shipped in the installed filesystem.
