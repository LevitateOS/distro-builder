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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn temp_dirs() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let staging = temp.path().join("staging");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&staging).unwrap();
        (temp, source, staging)
    }

    #[test]
    fn test_handle_writefile_creates_content() {
        let (_temp, _source, staging) = temp_dirs();

        handle_writefile(&staging, "etc/test-config.conf", "test-content-12345\nline two\n")
            .unwrap();

        let file_path = staging.join("etc/test-config.conf");
        assert!(file_path.exists(), "File should be created");

        let written = fs::read_to_string(&file_path).unwrap();
        assert_eq!(written, "test-content-12345\nline two\n");
    }

    #[test]
    fn test_handle_writefilemode_sets_permissions() {
        let (_temp, _source, staging) = temp_dirs();

        handle_writefilemode(&staging, "secret.txt", "secret content", 0o600).unwrap();

        let file_path = staging.join("secret.txt");
        assert!(file_path.exists());

        let metadata = fs::metadata(&file_path).unwrap();
        let permissions = metadata.permissions().mode();
        assert_eq!(permissions & 0o777, 0o600);
    }

    #[test]
    fn test_handle_symlink_creates_link() {
        let (_temp, _source, staging) = temp_dirs();

        handle_symlink(&staging, "bin", "usr/bin").unwrap();

        let link_path = staging.join("bin");
        assert!(link_path.is_symlink(), "Should be a symlink");

        let target = fs::read_link(&link_path).unwrap();
        assert_eq!(target.to_str().unwrap(), "usr/bin");
    }

    #[test]
    fn test_handle_symlink_overwrites_existing() {
        let (_temp, _source, staging) = temp_dirs();

        // Create initial symlink
        handle_symlink(&staging, "link", "original_target").unwrap();

        // Overwrite with new target
        handle_symlink(&staging, "link", "new_target").unwrap();

        let link_path = staging.join("link");
        let target = fs::read_link(&link_path).unwrap();
        assert_eq!(target.to_str().unwrap(), "new_target");
    }

    #[test]
    fn test_handle_copyfile_copies_file() {
        let (_temp, source, staging) = temp_dirs();

        // Create source file
        fs::create_dir_all(source.join("etc")).unwrap();
        fs::write(source.join("etc/config.txt"), "config content").unwrap();

        handle_copyfile(&source, &staging, "etc/config.txt").unwrap();

        let dst = staging.join("etc/config.txt");
        assert!(dst.exists(), "File should be copied");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "config content");
    }

    #[test]
    fn test_handle_copytree_copies_directory() {
        let (_temp, source, staging) = temp_dirs();

        // Create source directory tree
        fs::create_dir_all(source.join("etc/systemd")).unwrap();
        fs::write(source.join("etc/systemd/service.conf"), "service config").unwrap();

        handle_copytree(&source, &staging, "etc/systemd").unwrap();

        let dst = staging.join("etc/systemd/service.conf");
        assert!(dst.exists(), "File in tree should be copied");
        assert_eq!(fs::read_to_string(&dst).unwrap(), "service config");
    }

    #[test]
    fn test_handle_copyfile_missing_file() {
        let (_temp, source, staging) = temp_dirs();

        let result = handle_copyfile(&source, &staging, "nonexistent.txt");
        assert!(result.is_err(), "Should fail for missing file");
    }
}
