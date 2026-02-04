//! Component executor - interprets Op variants and performs actual operations.
//!
//! This module provides distro-agnostic operation handlers that can be used
//! by any distribution builder (LevitateOS, AcornOS, etc.).
//!
//! # Usage
//!
//! ```rust,ignore
//! use distro_builder::component::executor::{directories, files, users};
//! use std::path::Path;
//!
//! let staging = Path::new("/tmp/staging");
//!
//! // Create directories
//! directories::handle_dir(staging, "etc/myapp")?;
//!
//! // Write files
//! files::handle_writefile(staging, "etc/myapp/config", "key=value\n")?;
//!
//! // Create users
//! users::ensure_user(source, staging, "myuser", 1000, 1000, "/home/myuser", "/bin/bash")?;
//! ```

pub mod directories;
pub mod files;
pub mod users;

use std::path::Path;

/// Execute a generic operation that doesn't require distro-specific handling.
///
/// This function handles the basic operations that work the same way
/// across all distributions. Distro-specific operations (like systemd
/// unit enabling or OpenRC service setup) should be handled separately.
///
/// # Arguments
/// * `source` - Path to the source rootfs
/// * `staging` - Path to the staging directory
/// * `op` - The operation to execute (from Op enum)
///
/// # Returns
/// Ok(()) on success, or an error if the operation fails.
///
/// # Example
/// ```rust,ignore
/// use distro_builder::component::{Op, executor};
///
/// // Execute a directory creation operation
/// executor::execute_generic_op(source, staging, &Op::Dir("etc/config".into()))?;
/// ```
pub fn execute_generic_op(source: &Path, staging: &Path, op: &super::Op) -> anyhow::Result<()> {
    match op {
        // Directory operations
        super::Op::Dir(path) => directories::handle_dir(staging, path)?,
        super::Op::DirMode(path, mode) => directories::handle_dirmode(staging, path, *mode)?,
        super::Op::Dirs(paths) => {
            let paths_ref: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
            directories::handle_dirs(staging, &paths_ref)?;
        }

        // File operations
        super::Op::WriteFile(path, content) => {
            files::handle_writefile(staging, path, content)?;
        }
        super::Op::WriteFileMode(path, content, mode) => {
            files::handle_writefilemode(staging, path, content, *mode)?;
        }
        super::Op::Symlink(link, target) => {
            files::handle_symlink(staging, link, target)?;
        }
        super::Op::CopyFile(path) => {
            files::handle_copyfile(source, staging, path)?;
        }
        super::Op::CopyTree(path) => {
            files::handle_copytree(source, staging, path)?;
        }

        // User/group operations
        super::Op::User {
            name,
            uid,
            gid,
            home,
            shell,
        } => {
            users::handle_user(source, staging, name, *uid, *gid, home, shell)?;
        }
        super::Op::Group { name, gid } => {
            users::handle_group(source, staging, name, *gid)?;
        }

        // Binary operations - these need distro-specific handling for library deps
        super::Op::Bin(_) | super::Op::Sbin(_) | super::Op::Bins(_) | super::Op::Sbins(_) => {
            // These require distro-specific binary handling (library paths differ)
            anyhow::bail!("Binary operations require distro-specific handling");
        }

        // Custom operations - these are distro-specific and should be handled separately
        super::Op::Custom(_) => {
            anyhow::bail!("Custom operations require distro-specific handling");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
    fn test_execute_generic_op_dir() {
        let (_temp, source, staging) = temp_dirs();

        let op = super::super::Op::Dir("etc/test".into());
        execute_generic_op(&source, &staging, &op).unwrap();

        assert!(staging.join("etc/test").is_dir());
    }

    #[test]
    fn test_execute_generic_op_writefile() {
        let (_temp, source, staging) = temp_dirs();

        let op = super::super::Op::WriteFile("config.txt".into(), "hello world".into());
        execute_generic_op(&source, &staging, &op).unwrap();

        assert_eq!(
            fs::read_to_string(staging.join("config.txt")).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_execute_generic_op_user() {
        let (_temp, source, staging) = temp_dirs();
        fs::create_dir_all(staging.join("etc")).unwrap();

        let op = super::super::Op::User {
            name: "testuser".into(),
            uid: 1000,
            gid: 1000,
            home: "/home/testuser".into(),
            shell: "/bin/bash".into(),
        };
        execute_generic_op(&source, &staging, &op).unwrap();

        let passwd = fs::read_to_string(staging.join("etc/passwd")).unwrap();
        assert!(passwd.contains("testuser"));
    }

    #[test]
    fn test_execute_generic_op_bin_fails() {
        let (_temp, source, staging) = temp_dirs();

        let op = super::super::Op::Bin("ls".into());
        let result = execute_generic_op(&source, &staging, &op);

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("distro-specific"));
    }
}
