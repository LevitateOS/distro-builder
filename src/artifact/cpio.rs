//! CPIO archive creation for initramfs.
//!
//! Provides utilities for creating compressed cpio archives
//! used as initramfs images.

use anyhow::Result;
use std::path::Path;

use crate::process::shell;

/// Build a compressed cpio archive from a directory.
///
/// Creates a gzip-compressed cpio archive in newc format, suitable for
/// use as a Linux initramfs.
///
/// # Arguments
///
/// * `root` - Directory containing the initramfs contents
/// * `output` - Path for the output .cpio.gz file
/// * `gzip_level` - Gzip compression level (1-9, higher = smaller but slower)
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::cpio::build_cpio;
/// use std::path::Path;
///
/// build_cpio(
///     Path::new("/tmp/initramfs-root"),
///     Path::new("/tmp/initramfs.cpio.gz"),
///     6,
/// )?;
/// ```
pub fn build_cpio(root: &Path, output: &Path, gzip_level: u32) -> Result<()> {
    // Use find + cpio to create the archive
    // - find . -print0: List all files with null separator (handles special chars)
    // - cpio --null -o -H newc: Create archive in newc format (required for Linux initramfs)
    // - gzip -N: Compress with specified level
    let cpio_cmd = format!(
        "cd {} && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip -{} > {}",
        root.display(),
        gzip_level,
        output.display()
    );

    shell(&cpio_cmd)?;

    Ok(())
}
