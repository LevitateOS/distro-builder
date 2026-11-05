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

Canonical builds load ring-owned manifests from `distro-variants/<distro>/`: sources (Ring 3), filesystem products (Ring 2), artifact transforms (Ring 1), and releases (Ring 0). Build-host and scenario declarations are separate owners.

Build explicitly with `just release-build live-tools levitate`. Run an independent scenario with `just scenario-test live-tools levitate`; scenario commands consume existing artifacts. The workspace `architecture.md` describes product outputs, scenarios, and validation.

## Related Crates

- [distro-spec](https://github.com/LevitateOS/distro-spec) - Distribution specifications (constants, paths, services)
- [leviso](https://github.com/LevitateOS/leviso) - LevitateOS ISO builder
- [AcornOS](https://github.com/LevitateOS/AcornOS) - AcornOS ISO builder

## Migration Notes

- [docs/00_MIGRATION_INDEX.md](docs/00_MIGRATION_INDEX.md) - migration index for numbered distro-builder migration tracks
- [docs/01_MIGRATION_FEDORA_SWAP.md](docs/01_MIGRATION_FEDORA_SWAP.md) - active Fedora Server DVD swap plan for the LevitateOS Stage 01 source path
- [docs/02_MIGRATION_BOOTC.md](docs/02_MIGRATION_BOOTC.md) - cancelled track; the repo is keeping the current A/B model instead of migrating to `bootc`
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
