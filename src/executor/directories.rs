//! Directory operation handlers: Op::Dir, Op::DirMode, Op::Dirs
//!
//! These operations are distro-agnostic and work for any Linux distribution.

use anyhow::Result;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Handle Op::Dir: Create a directory
pub fn handle_dir(staging: &Path, path: &str) -> Result<()> {
    fs::create_dir_all(staging.join(path))?;
    Ok(())
}

/// Handle Op::DirMode: Create a directory with specific permissions
pub fn handle_dirmode(staging: &Path, path: &str, mode: u32) -> Result<()> {
    let full_path = staging.join(path);
    fs::create_dir_all(&full_path)?;
    fs::set_permissions(&full_path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

/// Handle Op::Dirs: Create multiple directories
pub fn handle_dirs(staging: &Path, paths: &[&str]) -> Result<()> {
    for path in paths {
        fs::create_dir_all(staging.join(path))?;
    }
    Ok(())
}
