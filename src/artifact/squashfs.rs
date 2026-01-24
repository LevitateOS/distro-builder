//! Squashfs image builder.
//!
//! Provides a wrapper around `mksquashfs` for creating compressed
//! filesystem images.
//!
//! # Status
//!
//! This is a placeholder module. The actual squashfs building logic
//! remains in leviso. This module defines the interface for future extraction.

use anyhow::Result;
use std::path::Path;

/// Options for building a squashfs image.
#[derive(Debug, Clone)]
pub struct SquashfsOptions<'a> {
    /// Compression algorithm (gzip, zstd, xz, lzo, lz4).
    ///
    /// Default: "gzip" (universal kernel compatibility)
    pub compression: &'a str,

    /// Block size (e.g., "128K", "256K", "512K", "1M").
    ///
    /// Larger blocks = better compression, more memory usage.
    /// Default: "1M"
    pub block_size: &'a str,

    /// Whether to include extended attributes.
    ///
    /// Default: false (simpler, more portable)
    pub xattrs: bool,
}

impl Default for SquashfsOptions<'_> {
    fn default() -> Self {
        Self {
            compression: "gzip",
            block_size: "1M",
            xattrs: false,
        }
    }
}

/// Build a squashfs image from a directory.
///
/// # Arguments
///
/// * `source_dir` - Directory to pack into squashfs
/// * `output` - Path for the output squashfs file
/// * `options` - Squashfs build options
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::squashfs::{build_squashfs, SquashfsOptions};
/// use std::path::Path;
///
/// let options = SquashfsOptions {
///     compression: "zstd",
///     ..Default::default()
/// };
///
/// build_squashfs(
///     Path::new("staging/"),
///     Path::new("output/filesystem.squashfs"),
///     &options,
/// )?;
/// ```
///
/// # Status
///
/// **UNIMPLEMENTED** - This is a placeholder. The actual implementation is in leviso.
pub fn build_squashfs(_source_dir: &Path, _output: &Path, _options: &SquashfsOptions) -> Result<()> {
    unimplemented!(
        "Squashfs building not yet extracted from leviso.\n\
         \n\
         To use squashfs building, use leviso directly or wait for\n\
         this functionality to be extracted."
    )
}
