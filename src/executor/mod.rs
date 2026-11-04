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

pub mod binaries;
pub mod directories;
pub mod files;
pub mod openrc;
pub mod users;

use crate::build::context::BuildContext;
use std::path::Path;

/// Execute a generic operation - BuildContext adapter version.
///
/// This is a wrapper for use with types that implement the BuildContext trait.
/// It delegates to execute_generic_op.
pub fn execute_generic_op_ctx<C>(ctx: &C, op: &super::Op) -> anyhow::Result<()>
where
    C: BuildContext,
{
    execute_generic_op(ctx.source(), ctx.staging(), op)
}

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
