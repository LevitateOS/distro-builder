//! Artifact builders for distribution images.
//!
//! This module provides utilities and wrappers for building:
//! - [`cpio`] - Compressed cpio archives for initramfs
//! - [`filesystem`] - Directory copying, initramfs structure creation
//! - [`iso_utils`] - ISO creation utilities (xorriso, checksums, EFI boot images)
//! - [`rootfs`] - Compressed filesystem images (EROFS or squashfs)
//! - [`initramfs`] - Initial RAM filesystem archives (trait definitions)
//! - [`iso`] - Bootable ISO images (trait definitions)
//!
//! # Usage
//!
//! The utility modules (`cpio`, `filesystem`, `iso_utils`) provide ready-to-use
//! functions that both LevitateOS and AcornOS can call directly.
//!
//! The trait modules (`initramfs`, `iso`, `rootfs`) define interfaces that
//! each distro implements with their specific configuration.

pub mod cpio;
pub mod disk;
pub mod filesystem;
pub mod initramfs;
pub mod iso;
pub mod iso_utils;
pub mod live_overlay;
pub mod rootfs;
