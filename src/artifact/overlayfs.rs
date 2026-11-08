//! Overlayfs payload image builder.
//!
//! Live overlay payloads are read-only EROFS images with the rootfs image's
//! settings.

use anyhow::Result;
use std::path::Path;

use crate::artifact::rootfs::{
    create_erofs, EROFS_CHUNK_SIZE, EROFS_COMPRESSION, EROFS_COMPRESSION_LEVEL,
};

/// Build a live overlay payload image with the rootfs image's EROFS settings.
pub fn build_overlayfs_default(source_dir: &Path, output: &Path) -> Result<()> {
    create_erofs(
        source_dir,
        output,
        EROFS_COMPRESSION,
        EROFS_COMPRESSION_LEVEL,
        EROFS_CHUNK_SIZE,
    )
}
