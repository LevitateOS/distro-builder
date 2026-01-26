//! Artifact builders for distribution images.
//!
//! This module provides utilities and wrappers for building:
//! - [`cpio`] - Compressed cpio archives for initramfs
//! - [`filesystem`] - Directory copying, initramfs structure creation
//! - [`iso_utils`] - ISO creation utilities (xorriso, checksums, EFI boot images)
//! - [`squashfs`] - Compressed filesystem images (mksquashfs)
//! - [`initramfs`] - Initial RAM filesystem archives (trait definitions)
//! - [`iso`] - Bootable ISO images (trait definitions)
//!
//! # Usage
//!
//! The utility modules (`cpio`, `filesystem`, `iso_utils`) provide ready-to-use
//! functions that both LevitateOS and AcornOS can call directly.
//!
//! The trait modules (`initramfs`, `iso`, `squashfs`) define interfaces that
//! each distro implements with their specific configuration.

pub mod cpio;
pub mod filesystem;
pub mod initramfs;
pub mod iso;
pub mod iso_utils;
pub mod squashfs;
