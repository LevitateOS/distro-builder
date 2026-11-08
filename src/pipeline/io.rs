use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use distro_contract::ProductId;

pub(crate) fn create_unique_output_dir(output_dir: &Path, logical_name: &Path) -> Result<PathBuf> {
    let stem = logical_name
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("rootfs-source");
    let path = output_dir.join(stem);
    if path.exists() {
        fs::remove_dir_all(&path).with_context(|| {
            format!(
                "removing existing product rootfs directory before recreation '{}'",
                path.display()
            )
        })?;
    }
    fs::create_dir_all(&path)
        .with_context(|| format!("creating product rootfs directory '{}'", path.display()))?;
    Ok(path)
}

pub(crate) fn create_empty_overlay_dir(output_dir: &Path, dir_name: &str) -> Result<PathBuf> {
    let overlay_dir = output_dir.join(dir_name);
    if overlay_dir.exists() {
        fs::remove_dir_all(&overlay_dir).with_context(|| {
            format!(
                "removing existing live overlay directory '{}'",
                overlay_dir.display()
            )
        })?;
    }
    fs::create_dir_all(&overlay_dir).with_context(|| {
        format!(
            "creating empty live overlay directory '{}'",
            overlay_dir.display()
        )
    })?;
    Ok(overlay_dir)
}

/// The rootfs image of the latest successful release of `product`.
pub fn resolve_release_product_rootfs_image_for_distro(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
    rootfs_filename: &str,
) -> Result<PathBuf> {
    let product_root = crate::run_history::release_product_dir(repo_root, distro_id, product);
    let run_id = crate::run_history::latest_successful_run_id(&product_root)?.ok_or_else(|| {
        anyhow::anyhow!(
            "{distro_id} has no successful {product} release under '{}'; build that release first",
            product_root.display(),
        )
    })?;
    let path = product_root.join(run_id).join(rootfs_filename);
    if !path.is_file() {
        bail!(
            "the latest {distro_id} {product} release has no rootfs image '{}'; build that release again",
            path.display(),
        );
    }
    Ok(path)
}

/// Whether the latest successful release of `product` has its rootfs image.
pub fn release_product_rootfs_exists_for_distro(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
    rootfs_filename: &str,
) -> Result<bool> {
    let product_root = crate::run_history::release_product_dir(repo_root, distro_id, product);
    let Some(run_id) = crate::run_history::latest_successful_run_id(&product_root)? else {
        return Ok(false);
    };
    Ok(product_root.join(run_id).join(rootfs_filename).is_file())
}

pub(crate) fn extract_erofs_rootfs(image: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination).with_context(|| {
            format!(
                "removing incomplete rootfs source directory '{}'",
                destination.display()
            )
        })?;
    }
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "creating rootfs source destination directory '{}'",
            destination.display()
        )
    })?;

    let extract_arg = format!("--extract={}", destination.display());
    let output = Command::new("fsck.erofs")
        .arg(extract_arg)
        // Keep staging writable; the embedded manifest is authoritative.
        .arg("--no-preserve-owner")
        .arg("--no-preserve-perms")
        .arg(image)
        .output()
        .with_context(|| format!("running fsck.erofs for '{}'", image.display()))?;

    if output.status.success() {
        use std::os::unix::fs::PermissionsExt;
        for entry in walkdir::WalkDir::new(destination).follow_links(false) {
            let entry = entry?;
            let meta = fs::symlink_metadata(entry.path())?;
            if meta.file_type().is_symlink() {
                continue;
            }
            let grant = if meta.is_dir() { 0o700 } else { 0o600 };
            fs::set_permissions(
                entry.path(),
                fs::Permissions::from_mode((meta.permissions().mode() & 0o777) | grant),
            )?;
        }
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "fsck.erofs failed extracting '{}' into '{}': {}\n{}",
        image.display(),
        destination.display(),
        stdout.trim(),
        stderr.trim()
    )
}
