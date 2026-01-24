//! Artifact builders for distribution images.
//!
//! This module provides wrappers for building:
//! - [`squashfs`] - Compressed filesystem images (mksquashfs)
//! - [`initramfs`] - Initial RAM filesystem archives (cpio + gzip)
//! - [`iso`] - Bootable ISO images (xorriso)
//!
//! # Status
//!
//! These are placeholder modules with interface definitions.
//! The actual implementations remain in leviso until they can be
//! properly abstracted and tested with both LevitateOS and AcornOS.

pub mod initramfs;
pub mod iso;
pub mod squashfs;
