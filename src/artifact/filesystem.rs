//! Filesystem utilities for artifact building.
//!
//! Common filesystem operations used during ISO, initramfs, and squashfs creation.

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
    if !dst.exists() {
        fs::create_dir_all(dst)
            .with_context(|| format!("Failed to create directory: {}", dst.display()))?;
    }

    for entry in fs::read_dir(src)
        .with_context(|| format!("Failed to read directory: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            // Preserve symlinks
            let target = fs::read_link(&src_path)?;
            if dst_path.exists() || dst_path.is_symlink() {
                fs::remove_file(&dst_path)?;
            }
            std::os::unix::fs::symlink(&target, &dst_path)
                .with_context(|| format!("Failed to create symlink: {}", dst_path.display()))?;
        } else if file_type.is_dir() {
            // Recurse into directories
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            // Copy regular files
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("Failed to copy file: {}", src_path.display()))?;
        }
    }

    Ok(())
}

/// Create initramfs directory structure.
///
/// Creates the minimal directory structure needed for a Linux initramfs:
/// - /bin, /sbin, /lib, /lib64
/// - /dev, /proc, /sys, /run
/// - /mnt, /tmp, /root
/// - /newroot (for switch_root target)
///
/// # Arguments
///
/// * `root` - Root directory for the initramfs
/// * `extra_dirs` - Additional directories to create (e.g., from distro-spec)
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::filesystem::create_initramfs_dirs;
/// use std::path::Path;
///
/// create_initramfs_dirs(
///     Path::new("/tmp/initramfs-root"),
///     &["media/cdrom", "overlay"],
/// )?;
/// ```
pub fn create_initramfs_dirs(root: &Path, extra_dirs: &[&str]) -> Result<()> {
    // Standard initramfs directories
    let standard_dirs = [
        "bin",
        "sbin",
        "lib",
        "lib64",
        "dev",
        "proc",
        "sys",
        "run",
        "mnt",
        "tmp",
        "root",
        "newroot",
    ];

    for dir in standard_dirs.iter().chain(extra_dirs.iter()) {
        fs::create_dir_all(root.join(dir))
            .with_context(|| format!("Failed to create directory: {}", dir))?;
    }

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
            fs::copy(src, dst)
                .with_context(|| format!("Failed to copy {} to {}", src.display(), dst.display()))?;
            fs::remove_file(src)
                .with_context(|| format!("Failed to remove {}", src.display()))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_copy_dir_recursive() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src");
        let dst = temp.path().join("dst");

        // Create source structure
        fs::create_dir_all(src.join("subdir")).unwrap();
        fs::write(src.join("file.txt"), "hello").unwrap();
        fs::write(src.join("subdir/nested.txt"), "world").unwrap();
        std::os::unix::fs::symlink("file.txt", src.join("link")).unwrap();

        // Copy
        copy_dir_recursive(&src, &dst).unwrap();

        // Verify
        assert!(dst.join("file.txt").exists());
        assert!(dst.join("subdir/nested.txt").exists());
        assert!(dst.join("link").is_symlink());
        assert_eq!(fs::read_link(dst.join("link")).unwrap().to_str().unwrap(), "file.txt");
    }

    #[test]
    fn test_create_initramfs_dirs() {
        let temp = TempDir::new().unwrap();
        let root = temp.path().join("initramfs");

        create_initramfs_dirs(&root, &["media/cdrom", "overlay"]).unwrap();

        assert!(root.join("bin").exists());
        assert!(root.join("dev").exists());
        assert!(root.join("proc").exists());
        assert!(root.join("media/cdrom").exists());
        assert!(root.join("overlay").exists());
    }

    #[test]
    fn test_atomic_move() {
        let temp = TempDir::new().unwrap();
        let src = temp.path().join("src.txt");
        let dst = temp.path().join("dst.txt");

        fs::write(&src, "content").unwrap();
        atomic_move(&src, &dst).unwrap();

        assert!(!src.exists());
        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "content");
    }
}
