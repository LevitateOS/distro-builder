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

pub mod artifact;
pub mod build;
pub mod component;
pub mod preflight;

pub use build::context::{BuildContext, DistroConfig, InitSystem};
pub use component::{Installable, Op, Phase};
