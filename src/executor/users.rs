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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn test_read_uid_from_rootfs() {
        let (_temp, source, _staging) = temp_dirs();

        fs::create_dir_all(source.join("etc")).unwrap();
        fs::write(
            source.join("etc/passwd"),
            "root:x:0:0:root:/root:/bin/bash\ndbus:x:81:81:System message bus:/:/sbin/nologin\n",
        )
        .unwrap();

        let root_uid = read_uid_from_rootfs(&source, "root").unwrap();
        assert_eq!(root_uid, Some((0, 0)));

        let dbus_uid = read_uid_from_rootfs(&source, "dbus").unwrap();
        assert_eq!(dbus_uid, Some((81, 81)));

        let missing = read_uid_from_rootfs(&source, "nonexistent").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn test_read_gid_from_rootfs() {
        let (_temp, source, _staging) = temp_dirs();

        fs::create_dir_all(source.join("etc")).unwrap();
        fs::write(source.join("etc/group"), "root:x:0:\ndbus:x:81:\n").unwrap();

        let root_gid = read_gid_from_rootfs(&source, "root").unwrap();
        assert_eq!(root_gid, Some(0));

        let dbus_gid = read_gid_from_rootfs(&source, "dbus").unwrap();
        assert_eq!(dbus_gid, Some(81));

        let missing = read_gid_from_rootfs(&source, "nonexistent").unwrap();
        assert_eq!(missing, None);
    }

    #[test]
    fn test_ensure_user_creates_entry() {
        let (_temp, source, staging) = temp_dirs();

        fs::create_dir_all(staging.join("etc")).unwrap();

        ensure_user(
            &source,
            &staging,
            "testuser",
            1000,
            1000,
            "/home/testuser",
            "/bin/bash",
        )
        .unwrap();

        let passwd_content = fs::read_to_string(staging.join("etc/passwd")).unwrap();
        assert!(passwd_content.contains("testuser:x:1000:1000:testuser:/home/testuser:/bin/bash"));
    }

    #[test]
    fn test_ensure_user_uses_source_uid() {
        let (_temp, source, staging) = temp_dirs();

        // Create source passwd with specific UID
        fs::create_dir_all(source.join("etc")).unwrap();
        fs::write(
            source.join("etc/passwd"),
            "testuser:x:1234:5678:::/bin/sh\n",
        )
        .unwrap();

        fs::create_dir_all(staging.join("etc")).unwrap();

        // Request with different defaults - should use source values
        ensure_user(
            &source,
            &staging,
            "testuser",
            9999,
            9999,
            "/home/test",
            "/bin/bash",
        )
        .unwrap();

        let passwd_content = fs::read_to_string(staging.join("etc/passwd")).unwrap();
        assert!(passwd_content.contains("testuser:x:1234:5678"));
    }

    #[test]
    fn test_ensure_user_idempotent() {
        let (_temp, source, staging) = temp_dirs();

        fs::create_dir_all(staging.join("etc")).unwrap();

        ensure_user(
            &source,
            &staging,
            "testuser",
            1000,
            1000,
            "/home/testuser",
            "/bin/bash",
        )
        .unwrap();
        ensure_user(
            &source,
            &staging,
            "testuser",
            1000,
            1000,
            "/home/testuser",
            "/bin/bash",
        )
        .unwrap();

        let passwd_content = fs::read_to_string(staging.join("etc/passwd")).unwrap();
        // Count lines that start with "testuser:" (full username match)
        let entry_count = passwd_content
            .lines()
            .filter(|line| line.starts_with("testuser:"))
            .count();
        assert_eq!(
            entry_count, 1,
            "Should only have one passwd entry for testuser"
        );
    }

    #[test]
    fn test_ensure_group_creates_entry() {
        let (_temp, source, staging) = temp_dirs();

        fs::create_dir_all(staging.join("etc")).unwrap();

        ensure_group(&source, &staging, "testgroup", 1000).unwrap();

        let group_content = fs::read_to_string(staging.join("etc/group")).unwrap();
        assert!(group_content.contains("testgroup:x:1000:"));
    }

    #[test]
    fn test_handle_user() {
        let (_temp, source, staging) = temp_dirs();

        fs::create_dir_all(staging.join("etc")).unwrap();

        handle_user(
            &source,
            &staging,
            "myuser",
            1001,
            1001,
            "/home/myuser",
            "/bin/sh",
        )
        .unwrap();

        let passwd_content = fs::read_to_string(staging.join("etc/passwd")).unwrap();
        assert!(passwd_content.contains("myuser:x:1001:1001:myuser:/home/myuser:/bin/sh"));
    }

    #[test]
    fn test_handle_group() {
        let (_temp, source, staging) = temp_dirs();

        fs::create_dir_all(staging.join("etc")).unwrap();

        handle_group(&source, &staging, "mygroup", 1001).unwrap();

        let group_content = fs::read_to_string(staging.join("etc/group")).unwrap();
        assert!(group_content.contains("mygroup:x:1001:"));
    }
}
