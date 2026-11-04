//! File operation handlers: Op::CopyFile, Op::CopyTree, Op::WriteFile, Op::WriteFileMode, Op::Symlink
//!
//! These operations are distro-agnostic and work for any Linux distribution.

use anyhow::{bail, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Handle Op::WriteFile: Write a file with content
pub fn handle_writefile(staging: &Path, path: &str, content: &str) -> Result<()> {
    let full_path = staging.join(path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full_path, content)?;
    Ok(())
}

/// Handle Op::WriteFileMode: Write a file with specific permissions
pub fn handle_writefilemode(staging: &Path, path: &str, content: &str, mode: u32) -> Result<()> {
    let full_path = staging.join(path);
    if let Some(parent) = full_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&full_path, content)?;
    fs::set_permissions(&full_path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Handle Op::Symlink: Create a symlink
///
/// If the symlink already exists, it will be removed and recreated.
/// This is important for cases where later components need to override
/// symlinks created by earlier components (e.g., /sbin/init).
pub fn handle_symlink(staging: &Path, link: &str, target: &str) -> Result<()> {
    let link_path = staging.join(link);
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Always overwrite existing symlinks - later components take precedence
    if link_path.is_symlink() || link_path.exists() {
        fs::remove_file(&link_path)?;
    }
    std::os::unix::fs::symlink(target, &link_path)?;
    Ok(())
}

/// Handle Op::CopyFile: Copy a file from source to staging
pub fn handle_copyfile(source: &Path, staging: &Path, path: &str) -> Result<()> {
    let src = source.join(path);
    let dst = staging.join(path);

    if !src.exists() {
        bail!("file not found: {}", src.display());
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&src, &dst)?;
    Ok(())
}

/// Handle Op::CopyTree: Copy an entire directory tree from source to staging
pub fn handle_copytree(source: &Path, staging: &Path, path: &str) -> Result<()> {
    let src = source.join(path);
    let dst = staging.join(path);

    if !src.exists() {
        bail!("directory not found: {}", src.display());
    }

    copy_dir_recursive(&src, &dst)?;
    Ok(())
}

/// Recursively copy a directory tree
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_symlink() {
            let target = fs::read_link(&src_path)?;
            std::os::unix::fs::symlink(target, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
