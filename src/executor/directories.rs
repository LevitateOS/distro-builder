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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn temp_staging() -> (TempDir, std::path::PathBuf) {
        let temp = TempDir::new().unwrap();
        let staging = temp.path().join("staging");
        fs::create_dir_all(&staging).unwrap();
        (temp, staging)
    }

    #[test]
    fn test_handle_dir_creates_nested_structure() {
        let (_temp, staging) = temp_staging();

        handle_dir(&staging, "var/lib/deeply/nested/directory").unwrap();

        let created_dir = staging.join("var/lib/deeply/nested/directory");
        assert!(created_dir.is_dir(), "Directory should be created");
    }

    #[test]
    fn test_handle_dirmode_sets_permissions() {
        let (_temp, staging) = temp_staging();

        handle_dirmode(&staging, "restricted_dir", 0o700).unwrap();

        let created_dir = staging.join("restricted_dir");
        assert!(created_dir.is_dir());

        let metadata = fs::metadata(&created_dir).unwrap();
        let permissions = metadata.permissions().mode();
        assert_eq!(permissions & 0o777, 0o700, "Permissions should be 0o700");
    }

    #[test]
    fn test_handle_dirs_creates_multiple() {
        let (_temp, staging) = temp_staging();

        let paths = ["etc", "var/log", "usr/bin", "tmp"];
        handle_dirs(&staging, &paths).unwrap();

        for path in &paths {
            assert!(staging.join(path).is_dir(), "{} should exist", path);
        }
    }

    #[test]
    fn test_handle_dir_idempotent() {
        let (_temp, staging) = temp_staging();

        // Create directory twice - should not fail
        handle_dir(&staging, "test_dir").unwrap();
        handle_dir(&staging, "test_dir").unwrap();

        assert!(staging.join("test_dir").is_dir());
    }
}
