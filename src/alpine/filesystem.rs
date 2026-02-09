//! Filesystem custom operations.
//!
//! Handles FHS symlinks and device manager setup.

use anyhow::Result;
use std::fs;

use super::context::BuildContext;

/// Create FHS symlinks for merged /usr.
///
/// Alpine uses merged /usr, so we create symlinks:
/// - /bin -> usr/bin
/// - /sbin -> usr/sbin
/// - /lib -> usr/lib
///
/// Note: musl doesn't use multilib, so we use /usr/lib (not /usr/lib64).
pub fn create_fhs_symlinks(ctx: &BuildContext) -> Result<()> {
    let staging = &ctx.staging;

    // Merged /usr symlinks
    let symlinks = [("bin", "usr/bin"), ("sbin", "usr/sbin"), ("lib", "usr/lib")];

    for (link, target) in symlinks {
        let link_path = staging.join(link);
        if !link_path.exists() && !link_path.is_symlink() {
            std::os::unix::fs::symlink(target, &link_path)?;
        }
    }

    // /var symlinks
    let var_symlinks = [("var/run", "/run"), ("var/lock", "/run/lock")];

    for (link, target) in var_symlinks {
        let link_path = staging.join(link);
        if !link_path.exists() && !link_path.is_symlink() {
            if let Some(parent) = link_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(target, &link_path)?;
        }
    }

    // Create /run/lock directory
    fs::create_dir_all(staging.join("run/lock"))?;

    Ok(())
}

/// Setup device manager (eudev or mdev).
///
/// Alpine-based distros use eudev (standalone udev fork) for device management
/// because mdev from busybox is too limited for production use.
pub fn setup_device_manager(ctx: &BuildContext) -> Result<()> {
    let staging = &ctx.staging;

    // Create device directories
    fs::create_dir_all(staging.join("dev"))?;
    fs::create_dir_all(staging.join("run/udev"))?;

    // Copy eudev rules if they exist in source
    let rules_src = ctx.source.join("etc/udev/rules.d");
    let rules_dst = staging.join("etc/udev/rules.d");
    if rules_src.exists() {
        copy_tree(&rules_src, &rules_dst)?;
    }

    // Copy default rules
    let lib_rules_src = ctx.source.join("usr/lib/udev/rules.d");
    let lib_rules_dst = staging.join("usr/lib/udev/rules.d");
    if lib_rules_src.exists() {
        fs::create_dir_all(&lib_rules_dst)?;
        copy_tree(&lib_rules_src, &lib_rules_dst)?;
    }

    // Copy udev helpers
    let helpers_src = ctx.source.join("usr/lib/udev");
    let helpers_dst = staging.join("usr/lib/udev");
    if helpers_src.exists() {
        fs::create_dir_all(&helpers_dst)?;
        for entry in fs::read_dir(&helpers_src)? {
            let entry = entry?;
            let path = entry.path();
            // Copy executables and rules directories
            if path.is_file() {
                let dst = helpers_dst.join(entry.file_name());
                fs::copy(&path, &dst)?;
            }
        }
    }

    Ok(())
}

/// Copy all shared libraries from source rootfs.
///
/// This is necessary because glibc's ldd can't analyze musl binaries,
/// so we can't detect library dependencies automatically. We copy ALL
/// shared libraries from the source rootfs to ensure binaries work.
pub fn copy_all_libraries(ctx: &BuildContext) -> Result<()> {
    let source = &ctx.source;
    let staging = &ctx.staging;

    // Library directories to copy
    let lib_dirs = ["lib", "usr/lib"];

    let mut copied = 0;
    for lib_dir in lib_dirs {
        let src_dir = source.join(lib_dir);
        if !src_dir.exists() {
            continue;
        }

        // Walk the directory and copy all .so files
        copy_libraries_recursive(&src_dir, &src_dir, staging, lib_dir, &mut copied)?;
    }

    println!("  Copied {} shared libraries", copied);
    Ok(())
}

/// Recursively copy shared libraries from a directory.
fn copy_libraries_recursive(
    base: &std::path::Path,
    current: &std::path::Path,
    staging: &std::path::Path,
    dest_prefix: &str,
    count: &mut usize,
) -> Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            // Recurse into subdirectories
            copy_libraries_recursive(base, &path, staging, dest_prefix, count)?;
        } else {
            let name = entry.file_name().to_string_lossy().to_string();

            // Copy .so files and symlinks
            if name.contains(".so") {
                // Calculate relative path from base
                let rel_path = path.strip_prefix(base).unwrap_or(&path);
                let dst_path = staging.join(dest_prefix).join(rel_path);

                // Create parent directory
                if let Some(parent) = dst_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                // Skip if already exists
                if dst_path.exists() || dst_path.is_symlink() {
                    continue;
                }

                if path.is_symlink() {
                    // Copy symlink
                    let target = fs::read_link(&path)?;
                    std::os::unix::fs::symlink(&target, &dst_path)?;
                } else {
                    // Copy file
                    fs::copy(&path, &dst_path)?;
                }
                *count += 1;
            }
        }
    }
    Ok(())
}

/// Copy a directory tree recursively.
fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }

    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(src, dst)?;
        return Ok(());
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_symlink() {
            let target = fs::read_link(&src_path)?;
            if dst_path.exists() || dst_path.is_symlink() {
                fs::remove_file(&dst_path)?;
            }
            std::os::unix::fs::symlink(&target, &dst_path)?;
        } else if src_path.is_dir() {
            copy_tree(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
