//! Filesystem utilities for artifact building.
//!
//! Common filesystem operations used during ISO, initramfs, and EROFS artifact creation.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Recursively copy a directory, preserving symlinks.
///
/// Unlike `fs::copy`, this properly handles:
/// - Nested directories
/// - Symbolic links (preserved, not followed)
/// - File permissions
///
/// This uses the overwrite variant from `leviso-elf`, which replaces existing
/// symlinks. This is the desired behavior for overlay creation.
///
/// # Arguments
///
/// * `src` - Source directory to copy
/// * `dst` - Destination directory (created if it doesn't exist)
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::filesystem::copy_dir_recursive;
/// use std::path::Path;
///
/// copy_dir_recursive(
///     Path::new("/tmp/source"),
///     Path::new("/tmp/dest"),
/// )?;
/// ```
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    // Use leviso-elf's overwrite variant (replaces existing symlinks)
    // and discard the byte count since callers expect Result<()>
    leviso_elf::copy_dir_recursive_overwrite(src, dst)?;
    Ok(())
}

/// Atomically move a file by renaming, with fallback to copy+delete.
///
/// Useful for the "atomic artifacts" pattern where we build to a temp file
/// and then atomically move to the final destination.
///
/// # Arguments
///
/// * `src` - Source file path
/// * `dst` - Destination file path
pub fn atomic_move(src: &Path, dst: &Path) -> Result<()> {
    // Try atomic rename first (works if same filesystem)
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Different filesystem, fall back to copy+delete
            fs::copy(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            fs::remove_file(src).with_context(|| format!("Failed to remove {}", src.display()))?;
            Ok(())
        }
    }
}

/// Writes `content` to `path`, creating its directory, with permission bits `mode`.
pub(crate) fn write_file(path: &Path, content: impl AsRef<[u8]>, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting the mode of '{}'", path.display()))
}

/// Copies the program at `source` to `destination` as an executable (0755).
pub(crate) fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
    let content = fs::read(source).with_context(|| format!("reading '{}'", source.display()))?;
    write_file(destination, content, 0o755)
}
