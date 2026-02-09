//! Binary operation handlers: find, copy, and library dependency resolution.
//!
//! These operations handle copying binaries from source rootfs to staging,
//! including resolving and copying shared library dependencies via ldd.

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::process::Cmd;

/// Find a binary in the source rootfs.
///
/// Searches usr/bin, bin, usr/sbin, sbin in order.
/// Returns the relative path to the binary if found.
pub fn find_binary(source: &Path, name: &str) -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("usr/bin").join(name),
        PathBuf::from("bin").join(name),
        PathBuf::from("usr/sbin").join(name),
        PathBuf::from("sbin").join(name),
    ];

    candidates
        .into_iter()
        .find(|candidate| source.join(candidate).exists())
}

/// Copy a binary from source to staging with its library dependencies.
///
/// Handles both regular binaries and symlinks (e.g. busybox applets).
/// For regular binaries, also copies shared library dependencies.
pub fn copy_binary(source: &Path, staging: &Path, name: &str, dest_dir: &str) -> Result<()> {
    // Find the binary in source
    let src_path = find_binary(source, name).ok_or_else(|| {
        let usr_bin = source.join("usr/bin").join(name);
        let bin = source.join("bin").join(name);
        anyhow::anyhow!(
            "binary not found: {} (checked {} [exists={}] and {} [exists={}])",
            name,
            usr_bin.display(),
            usr_bin.exists(),
            bin.display(),
            bin.exists()
        )
    })?;

    let src = source.join(&src_path);
    let dst = staging.join(dest_dir).join(name);

    // Create destination directory
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    // If it's a symlink (busybox applet), recreate the symlink
    if src.is_symlink() {
        let target = fs::read_link(&src)?;
        if dst.exists() || dst.is_symlink() {
            fs::remove_file(&dst)?;
        }
        std::os::unix::fs::symlink(&target, &dst)?;
        return Ok(());
    }

    // Remove existing file/symlink at destination (might be busybox applet)
    if dst.exists() || dst.is_symlink() {
        fs::remove_file(&dst)?;
    }

    // Copy the binary
    fs::copy(&src, &dst)
        .with_context(|| format!("copying {} to {}", src.display(), dst.display()))?;
    make_executable(&dst)?;

    // Copy library dependencies (musl-based)
    copy_library_deps(source, staging, &src)
        .with_context(|| format!("copying libs for {}", name))?;

    Ok(())
}

/// Copy library dependencies for a binary.
///
/// Uses ldd to find dependencies and copies them from source to staging.
pub fn copy_library_deps(source: &Path, staging: &Path, binary: &Path) -> Result<()> {
    let result = Cmd::new("ldd")
        .arg_path(binary)
        .allow_fail() // Some binaries (static) don't have deps - that's OK
        .run()
        .context("failed to run ldd")?;

    if !result.success() {
        // Static binary or ldd failed - no deps to copy
        return Ok(());
    }

    let stdout = &result.stdout;

    for line in stdout.lines() {
        if let Some(path) = extract_library_path(line) {
            let rel_path = path.strip_prefix('/').unwrap_or(&path);
            let src = source.join(rel_path);
            let dst = staging.join(rel_path);

            if src.exists() && !dst.exists() {
                if let Some(parent) = dst.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(&src, &dst)?;
            }
        }
    }

    Ok(())
}

/// Extract library path from ldd output line.
///
/// Parses format: "libfoo.so.1 => /usr/lib/libfoo.so.1 (0x...)"
pub fn extract_library_path(line: &str) -> Option<String> {
    if let Some(arrow_pos) = line.find("=>") {
        let after_arrow = &line[arrow_pos + 2..];
        let parts: Vec<&str> = after_arrow.split_whitespace().collect();
        if let Some(path) = parts.first() {
            if path.starts_with('/') {
                return Some(path.to_string());
            }
        }
    }
    None
}

/// Make a file executable (chmod +x).
pub fn make_executable(path: &Path) -> Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_library_path() {
        assert_eq!(
            extract_library_path("\tlibc.musl-x86_64.so.1 => /lib/ld-musl-x86_64.so.1 (0x7f...)"),
            Some("/lib/ld-musl-x86_64.so.1".to_string())
        );

        assert_eq!(extract_library_path("\tlinux-vdso.so.1"), None);
        assert_eq!(extract_library_path(""), None);
    }

    #[test]
    fn test_find_binary() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path();

        // Create a binary in usr/bin
        fs::create_dir_all(source.join("usr/bin")).unwrap();
        fs::write(source.join("usr/bin/test-bin"), "").unwrap();

        assert_eq!(
            find_binary(source, "test-bin"),
            Some(PathBuf::from("usr/bin/test-bin"))
        );
        assert_eq!(find_binary(source, "nonexistent"), None);
    }
}
