//! Busybox custom operations.
//!
//! Creates applet symlinks for busybox.

use anyhow::Result;
use std::fs;
use std::process::Command;

use super::context::BuildContext;

/// Create busybox applet symlinks.
///
/// Busybox provides many utilities through a single binary.
/// Each utility is accessed via a symlink to busybox.
///
/// The caller passes `sbin_applets` (which go in /usr/sbin) and
/// `common_applets` (fallback if `busybox --list` fails).
pub fn create_applet_symlinks(
    ctx: &BuildContext,
    sbin_applets: &[&str],
    common_applets: &[&str],
) -> Result<()> {
    let staging = &ctx.staging;

    // Find busybox in staging
    let busybox_path = staging.join("usr/bin/busybox");
    if !busybox_path.exists() {
        // Try to copy from source
        let src = ctx.source.join("bin/busybox");
        if src.exists() {
            fs::create_dir_all(staging.join("usr/bin"))?;
            fs::copy(&src, &busybox_path)?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&busybox_path, fs::Permissions::from_mode(0o755))?;
        } else {
            anyhow::bail!("busybox not found in source");
        }
    }

    // Get list of applets from busybox
    let applets = get_busybox_applets(&busybox_path, common_applets)?;

    // Create symlinks
    let bin_dir = staging.join("usr/bin");
    let sbin_dir = staging.join("usr/sbin");
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&sbin_dir)?;

    for applet in &applets {
        // Determine if this is a sbin command
        let (dir, target) = if sbin_applets.contains(&applet.as_str()) {
            (&sbin_dir, "/usr/bin/busybox")
        } else {
            (&bin_dir, "/usr/bin/busybox")
        };

        let link = dir.join(applet);

        // Don't overwrite existing files (might be standalone binaries)
        if !link.exists() && !link.is_symlink() {
            std::os::unix::fs::symlink(target, &link)?;
        }
    }

    // Create essential symlinks in /usr/bin that may be needed
    // Note: /bin is a symlink to /usr/bin (merged-usr), so we put these in usr/bin directly
    // The FHS symlinks are created by FILESYSTEM component before this runs
    {
        let name = "sh";
        let link = bin_dir.join(name);
        if !link.exists() && !link.is_symlink() {
            std::os::unix::fs::symlink("/usr/bin/busybox", &link)?;
        }
    }

    println!("  Created {} busybox applet symlinks", applets.len());

    Ok(())
}

/// Get list of busybox applets.
fn get_busybox_applets(
    busybox_path: &std::path::Path,
    common_applets: &[&str],
) -> Result<Vec<String>> {
    // Try running busybox --list
    match Command::new(busybox_path).arg("--list").output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect())
        }
        Ok(_) => {
            // Command ran but failed - use fallback
            Ok(common_applets.iter().map(|s| s.to_string()).collect())
        }
        Err(_) => {
            // Command failed to execute (e.g., musl binary on glibc system) - use fallback
            Ok(common_applets.iter().map(|s| s.to_string()).collect())
        }
    }
}
