//! User/group operation handlers: Op::User, Op::Group
//!
//! These operations are distro-agnostic and work for any Linux distribution.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Read a UID from the rootfs passwd file.
///
/// Returns:
/// - Ok(Some((uid, gid))) if user found
/// - Ok(None) if user not found or file doesn't exist
/// - Err if file exists but is corrupted/unreadable
pub fn read_uid_from_rootfs(rootfs: &Path, username: &str) -> Result<Option<(u32, u32)>> {
    let passwd_path = rootfs.join("etc/passwd");

    // File not existing is fine - user just doesn't exist
    if !passwd_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&passwd_path)
        .with_context(|| format!("Failed to read passwd file at {}", passwd_path.display()))?;

    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 4 && parts[0] == username {
            let uid: u32 = parts[2].parse().with_context(|| {
                format!(
                    "Corrupted passwd file: invalid UID '{}' for user '{}' at {}",
                    parts[2],
                    username,
                    passwd_path.display()
                )
            })?;
            let gid: u32 = parts[3].parse().with_context(|| {
                format!(
                    "Corrupted passwd file: invalid GID '{}' for user '{}' at {}",
                    parts[3],
                    username,
                    passwd_path.display()
                )
            })?;
            return Ok(Some((uid, gid)));
        }
    }
    Ok(None)
}

/// Read a GID from the rootfs group file.
///
/// Returns:
/// - Ok(Some(gid)) if group found
/// - Ok(None) if group not found or file doesn't exist
/// - Err if file exists but is corrupted/unreadable
pub fn read_gid_from_rootfs(rootfs: &Path, groupname: &str) -> Result<Option<u32>> {
    let group_path = rootfs.join("etc/group");

    // File not existing is fine - group just doesn't exist
    if !group_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&group_path)
        .with_context(|| format!("Failed to read group file at {}", group_path.display()))?;

    for line in content.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[0] == groupname {
            let gid: u32 = parts[2].parse().with_context(|| {
                format!(
                    "Corrupted group file: invalid GID '{}' for group '{}' at {}",
                    parts[2],
                    groupname,
                    group_path.display()
                )
            })?;
            return Ok(Some(gid));
        }
    }
    Ok(None)
}

/// Ensure a user exists in passwd file.
pub fn ensure_user(
    source: &Path,
    staging: &Path,
    username: &str,
    default_uid: u32,
    default_gid: u32,
    home: &str,
    shell: &str,
) -> Result<()> {
    let passwd_path = staging.join("etc/passwd");

    // Read existing passwd file
    // - If file doesn't exist, start with empty string (first user)
    // - If file exists but unreadable, FAIL FAST (don't silently overwrite)
    let mut passwd = if passwd_path.exists() {
        fs::read_to_string(&passwd_path)
            .with_context(|| format!("Failed to read passwd file at {}", passwd_path.display()))?
    } else {
        String::new()
    };

    if !passwd.contains(&format!("{}:", username)) {
        // Try to get UID/GID from source rootfs, fall back to defaults if user doesn't exist
        let (uid, gid) =
            read_uid_from_rootfs(source, username)?.unwrap_or((default_uid, default_gid));
        let entry = format!(
            "{}:x:{}:{}:{}:{}:{}\n",
            username, uid, gid, username, home, shell
        );
        passwd.push_str(&entry);
        fs::write(&passwd_path, passwd)
            .with_context(|| format!("Failed to write passwd for user {}", username))?;
    }
    Ok(())
}

/// Ensure a group exists in group file.
pub fn ensure_group(
    source: &Path,
    staging: &Path,
    groupname: &str,
    default_gid: u32,
) -> Result<()> {
    let group_path = staging.join("etc/group");

    // Read existing group file
    // - If file doesn't exist, start with empty string (first group)
    // - If file exists but unreadable, FAIL FAST (don't silently overwrite)
    let mut group = if group_path.exists() {
        fs::read_to_string(&group_path)
            .with_context(|| format!("Failed to read group file at {}", group_path.display()))?
    } else {
        String::new()
    };

    if !group.contains(&format!("{}:", groupname)) {
        // Try to get GID from source rootfs, fall back to default if group doesn't exist
        let gid = read_gid_from_rootfs(source, groupname)?.unwrap_or(default_gid);
        let entry = format!("{}:x:{}:\n", groupname, gid);
        group.push_str(&entry);
        fs::write(&group_path, group)
            .with_context(|| format!("Failed to write group for {}", groupname))?;
    }
    Ok(())
}

/// Handle Op::User: Create or update a user
pub fn handle_user(
    source: &Path,
    staging: &Path,
    name: &str,
    uid: u32,
    gid: u32,
    home: &str,
    shell: &str,
) -> Result<()> {
    ensure_user(source, staging, name, uid, gid, home, shell)
}

/// Handle Op::Group: Create or update a group
pub fn handle_group(source: &Path, staging: &Path, name: &str, gid: u32) -> Result<()> {
    ensure_group(source, staging, name, gid)
}
