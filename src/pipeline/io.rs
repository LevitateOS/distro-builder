use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn create_unique_output_dir(output_dir: &Path, logical_name: &Path) -> Result<PathBuf> {
    let stem = logical_name
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("sxx-rootfs-source");
    let path = output_dir.join(stem);
    if path.exists() {
        fs::remove_dir_all(&path).with_context(|| {
            format!(
                "removing existing stage rootfs directory before recreation '{}'",
                path.display()
            )
        })?;
    }
    fs::create_dir_all(&path)
        .with_context(|| format!("creating stage rootfs directory '{}'", path.display()))?;
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

pub(crate) fn rename_live_overlay_dir(
    output_dir: &Path,
    source_overlay: &Path,
    target_dir_name: &str,
) -> Result<PathBuf> {
    let target_overlay = output_dir.join(target_dir_name);
    if source_overlay == target_overlay {
        return Ok(target_overlay);
    }
    if target_overlay.exists() {
        fs::remove_dir_all(&target_overlay).with_context(|| {
            format!(
                "removing pre-existing stage live overlay '{}'",
                target_overlay.display()
            )
        })?;
    }
    fs::rename(source_overlay, &target_overlay).with_context(|| {
        format!(
            "renaming live overlay '{}' -> '{}'",
            source_overlay.display(),
            target_overlay.display()
        )
    })?;
    Ok(target_overlay)
}

pub(crate) fn resolve_parent_product_rootfs_image_for_distro(
    repo_root: &Path,
    distro_id: &str,
    product_dir_name: &str,
    parent_product_label: &str,
    rootfs_filename: &str,
) -> Result<PathBuf> {
    let product_root = repo_root
        .join(".artifacts")
        .join("out")
        .join(distro_id)
        .join("releases")
        .join(product_dir_name);

    let run_id = crate::run_history::latest_successful_run_id(&product_root)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing successful {} release under '{}'; build parent product first",
            parent_product_label,
            product_root.display()
        )
    })?;
    let path = product_root.join(run_id).join(rootfs_filename);
    if !path.is_file() {
        bail!(
            "missing parent product rootfs image '{}'; build parent product first",
            path.display()
        );
    }
    Ok(path)
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
        .arg(image)
        .output()
        .with_context(|| format!("running fsck.erofs for '{}'", image.display()))?;

    if output.status.success() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_parent_product_rootfs_image_for_distro_uses_repo_layout_not_output_dir_ancestry() {
        let repo_root = tempfile::tempdir().expect("repo tempdir");
        let stage_root = repo_root
            .path()
            .join(".artifacts/out/levitate/releases/base-rootfs/run-1");
        fs::create_dir_all(&stage_root).expect("create stage root");
        fs::write(
            crate::run_history::manifest_path(&stage_root),
            serde_json::to_vec_pretty(&json!({
                "run_id": "run-1",
                "status": "success",
                "created_at_utc": "20260313T120000Z",
                "finished_at_utc": "20260313T120001Z",
            }))
            .expect("serialize manifest"),
        )
        .expect("write run manifest");
        let rootfs = stage_root.join("filesystem.erofs");
        fs::write(&rootfs, b"test rootfs").expect("write rootfs file");

        let resolved = resolve_parent_product_rootfs_image_for_distro(
            repo_root.path(),
            "levitate",
            "base-rootfs",
            "base-rootfs",
            "filesystem.erofs",
        )
        .expect("resolve parent product rootfs");

        assert_eq!(resolved, rootfs);
    }
}
