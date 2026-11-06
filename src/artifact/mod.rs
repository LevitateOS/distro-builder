//! Artifact builders for distribution images.
//!
//! This module provides utilities and wrappers for building:
//! - [`filesystem`] - Directory copying
//! - [`iso_utils`] - ISO creation utilities (xorriso, checksums, EFI boot images)
//! - [`rootfs`] - Compressed filesystem images (EROFS)
//! - [`overlayfs`] - Live overlay payload images (EROFS)
//! - [`iso`] - Bootable ISO images (trait definitions)
//!
//! # Usage
//!
//! The utility modules (`filesystem`, `iso_utils`) provide ready-to-use
//! functions that both LevitateOS and AcornOS can call directly.
//!
//! The trait modules (`iso`, `rootfs`) define interfaces that
//! each distro implements with their specific configuration.

pub mod disk;
pub mod filesystem;
pub mod iso;
pub mod iso_utils;
pub mod live_overlay;
pub mod overlayfs;
pub mod rootfs;
