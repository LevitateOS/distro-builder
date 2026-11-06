//! Overlayfs payload image builder.
//!
//! Live overlay payloads are first-class filesystem artifacts represented as
//! read-only EROFS images. This module provides the canonical builder helpers.

use anyhow::Result;
use std::path::Path;

use crate::artifact::rootfs::create_erofs;

/// Build a live overlay payload image as EROFS with explicit settings.
pub fn create_overlayfs_erofs(
    source_dir: &Path,
    output: &Path,
    compression: &str,
    compression_level: u8,
    chunk_size: u32,
) -> Result<()> {
    create_erofs(
        source_dir,
        output,
        compression,
        compression_level,
        chunk_size,
    )
}

/// Build a live overlay payload image with the rootfs image's EROFS settings.
pub fn build_overlayfs_default(source_dir: &Path, output: &Path) -> Result<()> {
    use super::rootfs::{EROFS_CHUNK_SIZE, EROFS_COMPRESSION, EROFS_COMPRESSION_LEVEL};

    create_overlayfs_erofs(
        source_dir,
        output,
        EROFS_COMPRESSION,
        EROFS_COMPRESSION_LEVEL,
        EROFS_CHUNK_SIZE,
    )
}
