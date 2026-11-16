//! EROFS rootfs image builder.
//!
//! Provides the shared EROFS implementation used by both LevitateOS and AcornOS.
//! This is the **only** rootfs format supported for output artifacts.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;


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

    crate::tprintln!(
        "Creating EROFS with {} compression (level {})...",
        compression, compression_level
    );

    // Format compression argument: algorithm,level
    let compression_arg = format!("{},{}", compression, compression_level);

    // Target ownership/modes/xattrs travel in tar headers, independently of
    // staging privileges. Never bake host ownership or staging read grants in.
    // The tar streams straight into mkfs.erofs (no source argument: it reads
    // stdin). No temporary tar is written, synced and read back: on a desktop
    // tree that was 3.3 GB forced to disk and read twice more, and the image
    // step was bound by that I/O, not by compression. With a fixed UUID and
    // time the image is the same, byte for byte, as one made from a file.
    let mut mkfs = std::process::Command::new("mkfs.erofs")
        .args(["-z", &compression_arg])
        .args(["-C", &chunk_size.to_string()])
        .arg("--tar=f")
        .arg(format!("-T{IMAGE_TIMESTAMP}"))
        .args(["-U", &image_uuid(source_dir, output)?])
        .arg(output)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("starting mkfs.erofs")?;
    let image_input = mkfs.stdin.take().context("mkfs.erofs has no stdin")?;
    let written = (|| -> Result<()> {
        let mut input = std::io::BufWriter::new(image_input);
        levitate_recipe::target::write_tar(source_dir, &mut input)?;
        std::io::Write::flush(&mut input)?;
        // Closing the pipe signals end of stream to mkfs.erofs.
        drop(input);
        Ok(())
    })();
    let status = mkfs.wait().context("waiting for mkfs.erofs")?;
    if !status.success() {
        bail!("mkfs.erofs importing the rootfs tar failed: {status}");
    }
    written.context("streaming the rootfs tar into mkfs.erofs")?;

    // Print size
    let metadata = fs::metadata(output)?;
    let bytes = metadata.len();
    crate::tprintln!("EROFS created: {}", format_size_human(bytes));

    Ok(())
}

/// The filesystem UUID of the image `output` made from `source_dir`. mkfs.erofs
/// picks a random one otherwise, and that alone made two images of one tree
/// differ. Derived from the tree's target metadata manifest (every file's
/// digest) and the image's name, so different images never share one.
fn image_uuid(source_dir: &Path, output: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"ginux-erofs-uuid\0");
    if let Ok(manifest) = fs::read(source_dir.join(levitate_recipe::target::MANIFEST)) {
        hash.update(&manifest);
    }
    hash.update([0]);
    hash.update(output.file_name().map(|name| name.as_encoded_bytes()).unwrap_or_default());
    let b = hash.finalize();
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
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

/// The one modification time every inode of an image gets: fixed, so images
/// stay reproducible, but not the Unix epoch: programs that reread a file only
/// when it is newer than what they last read can treat files dated 1970-01-01
/// as never changed (a display manager once ignored every configuration
/// drop-in this way). 2026-01-01 00:00:00 UTC.
pub(crate) const IMAGE_TIMESTAMP: u64 = 1_767_225_600;

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
