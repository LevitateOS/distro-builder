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
    │   ├── squashfs   mksquashfs wrapper
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

## Status

This crate is currently a **structural skeleton**. The abstractions are defined but artifact builders contain placeholder implementations. Full extraction from leviso requires testing with both LevitateOS and AcornOS builds.

### Implemented
- `Installable` trait and `Phase` enum
- Generic `Op` variants (directory, file, symlink, user/group, binary)
- `DistroConfig` and `BuildContext` traits
- FHS filesystem utilities (with tests)
- Host tool preflight validation

### Placeholder (future work)
- Squashfs builder (interface only)
- Initramfs builder (interface only)
- ISO builder (interface only)

## Related Crates

- [distro-spec](https://github.com/LevitateOS/distro-spec) - Distribution specifications (constants, paths, services)
- [leviso](https://github.com/LevitateOS/leviso) - LevitateOS ISO builder
- [AcornOS](https://github.com/LevitateOS/AcornOS) - AcornOS ISO builder

## License

Licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
