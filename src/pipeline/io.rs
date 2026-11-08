use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::process::Cmd;

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

/// The rootfs image of the latest successful release of `product`; `None`
/// when there is no successful release or it has no rootfs image.
pub fn latest_release_rootfs(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
    rootfs_filename: &str,
) -> Result<Option<PathBuf>> {
    let release_dir = distro_contract::runs::release_dir(repo_root, distro_id, product);
    Ok(distro_contract::runs::latest_success(&release_dir)?
        .map(|(dir, _)| dir.join(rootfs_filename))
        .filter(|path| path.is_file()))
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

    let mut extract = std::ffi::OsString::from("--extract=");
    extract.push(destination);
    Cmd::new("fsck.erofs")
        .arg(extract)
        // Keep staging writable; the embedded manifest is authoritative.
        .arg("--no-preserve-owner")
        .arg("--no-preserve-perms")
        .arg(image)
        .error_msg(format!(
            "fsck.erofs extracting '{}' into '{}'",
            image.display(),
            destination.display()
        ))
        .run()?;
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
    Ok(())
}
