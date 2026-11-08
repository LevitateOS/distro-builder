//! EROFS rootfs image builder.
//!
//! Provides the shared EROFS implementation used by both LevitateOS and AcornOS.
//! This is the **only** rootfs format supported for output artifacts.
//!
//! # Why EROFS Only?
//!
//! - EROFS is the modern format (Fedora 42+, RHEL 10, Android)
//! - Better random-access performance than squashfs
//! - Lower memory overhead during decompression
//! - Actively developed in Linux kernel
//!
//! # Example
//!
//! ```rust,ignore
//! use distro_builder::artifact::rootfs::create_erofs;
//! use std::path::Path;
//!
//! // Build EROFS with custom settings
//! create_erofs(
//!     Path::new("staging/"),
//!     Path::new("output/filesystem.erofs"),
//!     "zstd",
//!     6,
//!     1048576,
//! )?;
//!
//! // Or use the default settings
//! use distro_builder::build_erofs_default;
//! build_erofs_default(Path::new("staging/"), Path::new("output/filesystem.erofs"))?;
//! ```
//!
//! # Note on Squashfs
//!
//! Squashfs support is intentionally NOT provided here. Both LevitateOS and
//! AcornOS use EROFS for their rootfs. Legacy squashfs inputs may still exist
//! in upstream ecosystems, but this builder does not emit squashfs artifacts.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

use crate::process::Cmd;

/// Create an EROFS image from a directory.
///
/// This is the shared implementation used by both LevitateOS and AcornOS.
///
/// # Arguments
///
/// * `source_dir` - Directory to pack into the EROFS image (must exist)
/// * `output` - Path for the output image file
/// * `compression` - Compression algorithm ("zstd", "lz4", "lzma", "deflate")
/// * `compression_level` - Compression level (1-22 for zstd)
/// * `chunk_size` - Chunk size in bytes (e.g., 1048576 for 1MB)
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::rootfs::create_erofs;
/// use std::path::Path;
///
/// create_erofs(
///     Path::new("staging/"),
///     Path::new("output/filesystem.erofs"),
///     "zstd",
///     6,
///     1048576,
/// )?;
/// ```
pub fn create_erofs(
    source_dir: &Path,
    output: &Path,
    compression: &str,
    compression_level: u8,
    chunk_size: u32,
) -> Result<()> {
    // Validate source directory
    if !source_dir.exists() {
        bail!("Source directory does not exist: {}", source_dir.display());
    }
    if !source_dir.is_dir() {
        bail!("Source path is not a directory: {}", source_dir.display());
    }

    // Ensure output directory exists
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create output directory: {}", parent.display()))?;
    }

    println!(
        "Creating EROFS with {} compression (level {})...",
        compression, compression_level
    );

    // Format compression argument: algorithm,level
    let compression_arg = format!("{},{}", compression, compression_level);

    // Target ownership/modes/xattrs travel in tar headers, independently of
    // staging privileges. Never bake host ownership or staging read grants in.
    let mut archive = tempfile::NamedTempFile::new_in(output.parent().unwrap_or(Path::new(".")))?;
    levitate_recipe::target::write_tar(source_dir, archive.as_file_mut())?;
    archive.as_file().sync_all()?;
    Cmd::new("mkfs.erofs")
        .args(["-z", &compression_arg])
        .args(["-C", &chunk_size.to_string()])
        .arg("--tar=f")
        .arg("-T0")
        .arg(output)
        .arg(archive.path())
        .error_msg("mkfs.erofs importing the rootfs tar")
        .run_interactive()?;

    // Print size
    let metadata = fs::metadata(output)?;
    let bytes = metadata.len();
    println!("EROFS created: {}", format_size_human(bytes));

    Ok(())
}

fn format_size_human(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }

    if value >= 10.0 || (value.fract() - 0.0).abs() < f64::EPSILON {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// EROFS settings for the rootfs and live overlay images.
pub(crate) const EROFS_COMPRESSION: &str = "zstd";
pub(crate) const EROFS_COMPRESSION_LEVEL: u8 = 6;
pub(crate) const EROFS_CHUNK_SIZE: u32 = 1 << 20;

/// Build an EROFS image with the default settings above.
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::build_erofs_default;
/// use std::path::Path;
///
/// build_erofs_default(
///     Path::new("staging/"),
///     Path::new("output/filesystem.erofs"),
/// )?;
/// ```
pub fn build_erofs_default(source_dir: &Path, output: &Path) -> Result<()> {
    create_erofs(
        source_dir,
        output,
        EROFS_COMPRESSION,
        EROFS_COMPRESSION_LEVEL,
        EROFS_CHUNK_SIZE,
    )
}
