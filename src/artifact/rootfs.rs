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
//! // Or use distro-spec defaults
//! use distro_builder::build_erofs_default;
//! build_erofs_default(Path::new("staging/"), Path::new("output/filesystem.erofs"))?;
//! ```
//!
//! # Note on Squashfs
//!
//! Squashfs support is intentionally NOT provided here. Both LevitateOS and
//! AcornOS use EROFS for their rootfs. Squashfs is only used for reading
//! upstream distribution media (Rocky's install.img, Alpine's modloop).

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

use crate::process::{self, Cmd};

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
        bail!(
            "Source directory does not exist: {}",
            source_dir.display()
        );
    }
    if !source_dir.is_dir() {
        bail!(
            "Source path is not a directory: {}",
            source_dir.display()
        );
    }

    // Check tool availability
    if !process::exists("mkfs.erofs") {
        bail!(
            "mkfs.erofs not found. Install erofs-utils:\n\
             On Fedora: sudo dnf install erofs-utils\n\
             On Ubuntu: sudo apt install erofs-utils\n\
             On Arch: sudo pacman -S erofs-utils\n\
             \n\
             NOTE: erofs-utils 1.5+ required for lz4hc, 1.8+ for zstd."
        );
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

    // IMPORTANT: mkfs.erofs argument order is OUTPUT SOURCE (opposite of mksquashfs!)
    Cmd::new("mkfs.erofs")
        .args(["-z", &compression_arg])
        .args(["-C", &chunk_size.to_string()])
        .arg("--all-root") // All files owned by root (required for sshd, etc.)
        .arg("-T0") // Reproducible builds (timestamp=0)
        .arg_path(output) // OUTPUT FIRST
        .arg_path(source_dir) // SOURCE SECOND
        .error_msg(
            "mkfs.erofs failed. Install erofs-utils: sudo dnf install erofs-utils\n\
             NOTE: erofs-utils 1.5+ required for lz4hc, 1.8+ for zstd.",
        )
        .run_interactive()?;

    // Print size
    let metadata = fs::metadata(output)?;
    println!("EROFS created: {} MB", metadata.len() / 1024 / 1024);

    Ok(())
}

/// Build an EROFS image using distro-spec default settings.
///
/// Uses constants from `distro_spec::shared::rootfs`:
/// - Compression: zstd
/// - Level: 6
/// - Chunk size: 1MB
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
    use distro_spec::shared::rootfs::{EROFS_CHUNK_SIZE, EROFS_COMPRESSION, EROFS_COMPRESSION_LEVEL};

    create_erofs(
        source_dir,
        output,
        EROFS_COMPRESSION,
        EROFS_COMPRESSION_LEVEL,
        EROFS_CHUNK_SIZE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_dir_validation() {
        let result = create_erofs(
            Path::new("/nonexistent_path_12345"),
            Path::new("/tmp/test.erofs"),
            "zstd",
            6,
            1048576,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not exist"));
    }

    #[test]
    fn test_source_not_directory() {
        // /dev/null exists but is not a directory
        let result = create_erofs(
            Path::new("/dev/null"),
            Path::new("/tmp/test.erofs"),
            "zstd",
            6,
            1048576,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not a directory"));
    }
}
