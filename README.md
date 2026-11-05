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
