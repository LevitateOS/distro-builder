//! Shared infrastructure for building Linux distribution ISOs.
//!
//! This crate provides common abstractions used by both leviso (LevitateOS)
//! and AcornOS builders. It extracts the distro-agnostic parts:
//!
//! - **Component system** - Declarative component definitions with traits and operations
//! - **Artifact builders** - Squashfs, initramfs, and ISO creation wrappers
//! - **Build utilities** - Filesystem operations and context management
//! - **Preflight checks** - Host tool validation before builds
//!
//! # Architecture
//!
//! ```text
//! distro-builder (this crate)
//!     │
//!     ├── Defines: Installable trait, generic Op variants
//!     ├── Defines: BuildContext trait, DistroConfig trait
//!     └── Provides: Filesystem utilities, squashfs/ISO wrappers
//!
//! leviso ─────────────────────┐
//!     │                       │
//!     ├── Uses: distro-builder│
//!     ├── Implements: LevitateOS-specific components
//!     └── Uses: distro-spec::levitate
//!
//! AcornOS ────────────────────┤
//!     │                       │
//!     ├── Uses: distro-builder│
//!     ├── Implements: AcornOS-specific components
//!     └── Uses: distro-spec::acorn
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use distro_builder::component::{Installable, Op, Phase};
//!
//! struct NetworkComponent;
//!
//! impl Installable for NetworkComponent {
//!     fn name(&self) -> &str { "Network" }
//!     fn phase(&self) -> Phase { Phase::Services }
//!     fn ops(&self) -> Vec<Op> {
//!         vec![
//!             Op::Dir("etc/network".into()),
//!             Op::WriteFile("etc/network/interfaces".into(), "auto lo\n".into()),
//!         ]
//!     }
//! }
//! ```
//!
//! # Status
//!
//! This crate is currently a structural skeleton. The abstractions are defined
//! but not all functionality is extracted from leviso yet. Full extraction
//! requires testing with both LevitateOS and AcornOS builds.

pub mod alpine;
pub mod artifact;
pub mod build;
pub mod cache;
pub mod component;
pub mod executor;
pub mod preflight;
pub mod process;
pub mod qemu;
pub mod recipe;

pub use build::context::{BuildContext, DistroConfig, InitSystem, PackageManager};
pub use build::kernel::{KernelBuildGuard, KernelGuard, KernelInstallConfig};
pub use build::licenses::LicenseTracker;
pub use component::{Installable, Op, Phase};
pub use executor::{binaries, directories, files, openrc, users};

// Re-export commonly used artifact utilities
pub use artifact::cpio::build_cpio;
pub use artifact::disk::{
    build_disk_image, build_disk_image_with_uuids, generate_disk_uuids, DiskImageConfig, DiskUuids,
};
pub use artifact::filesystem::{atomic_move, copy_dir_recursive, create_initramfs_dirs};
pub use artifact::iso_utils::{
    create_efi_boot_image, create_efi_dirs_in_fat, create_fat16_image, generate_iso_checksum,
    mcopy_to_fat, run_xorriso, setup_iso_structure,
};
pub use artifact::live_overlay::{create_openrc_live_overlay, InittabVariant, LiveOverlayConfig};
pub use artifact::rootfs::{build_erofs_default, create_erofs};

// Re-export process utilities
pub use process::{ensure_exists, find_first_existing, Cmd, CommandResult};
