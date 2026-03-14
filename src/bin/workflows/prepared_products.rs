use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub(crate) struct PreparedProductInputs {
    pub(crate) rootfs_source_dir: PathBuf,
    pub(crate) live_overlay_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PreparedProductManifest {
    pub(crate) product: String,
    pub(crate) distro_id: String,
    pub(crate) rootfs_source_dir: String,
    pub(crate) live_overlay_dir: String,
    pub(crate) rootfs_source_pointer_filename: String,
    pub(crate) rootfs_erofs_filename: String,
    pub(crate) overlay_erofs_filename: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedOutputNames {
    pub(crate) rootfs_source_pointer_filename: String,
    pub(crate) rootfs_erofs_filename: String,
    pub(crate) overlay_erofs_filename: String,
}

pub(crate) fn canonical_prepared_output_names(product: crate::BuildProduct) -> PreparedOutputNames {
    PreparedOutputNames {
        rootfs_source_pointer_filename: product.rootfs_source_pointer_filename.to_string(),
        rootfs_erofs_filename: product.rootfs_erofs_filename.to_string(),
        overlay_erofs_filename: product.overlay_erofs_filename.to_string(),
    }
}

pub(crate) fn compatibility_prepared_output_names(
    stage: crate::CompatibilityBuildStage,
) -> PreparedOutputNames {
    PreparedOutputNames {
        rootfs_source_pointer_filename: format!(".{}-live-rootfs-source.path", stage.artifact_tag),
        rootfs_erofs_filename: format!("{}-filesystem.erofs", stage.artifact_tag),
        overlay_erofs_filename: format!("{}-overlayfs.erofs", stage.artifact_tag),
    }
}

pub(crate) fn write_prepared_product_outputs(
    output_dir: &Path,
    product: crate::BuildProduct,
    distro_id: &str,
    prepared: &PreparedProductInputs,
    output_names: &PreparedOutputNames,
) -> Result<PathBuf> {
    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file = output_dir.join(&output_names.rootfs_source_pointer_filename);
    fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing product '{}' rootfs source path file '{}'",
            product.canonical,
            source_path_file.display()
        )
    })?;
    write_prepared_product_manifest(
        output_dir,
        &PreparedProductManifest {
            product: product.canonical.to_string(),
            distro_id: distro_id.to_string(),
            rootfs_source_dir: relative_prepared_product_path(
                output_dir,
                &prepared.rootfs_source_dir,
            )?,
            live_overlay_dir: relative_prepared_product_path(
                output_dir,
                &prepared.live_overlay_dir,
            )?,
            rootfs_source_pointer_filename: output_names.rootfs_source_pointer_filename.clone(),
            rootfs_erofs_filename: output_names.rootfs_erofs_filename.clone(),
            overlay_erofs_filename: output_names.overlay_erofs_filename.clone(),
        },
    )?;
    Ok(source_path_file)
}

pub(crate) fn read_prepared_product_manifest(
    prepared_dir: &Path,
) -> Result<PreparedProductManifest> {
    let manifest_path = prepared_product_manifest_path(prepared_dir);
    let bytes = fs::read(&manifest_path).with_context(|| {
        format!(
            "reading prepared product manifest '{}'",
            manifest_path.display()
        )
    })?;
    serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "parsing prepared product manifest '{}'",
            manifest_path.display()
        )
    })
}

pub(crate) fn resolve_prepared_product_path(prepared_dir: &Path, relative: &str) -> PathBuf {
    prepared_dir.join(relative)
}

fn prepared_product_manifest_path(output_dir: &Path) -> PathBuf {
    output_dir.join(".prepared-product.json")
}

fn write_prepared_product_manifest(
    output_dir: &Path,
    manifest: &PreparedProductManifest,
) -> Result<()> {
    let manifest_path = prepared_product_manifest_path(output_dir);
    let bytes =
        serde_json::to_vec_pretty(manifest).context("serializing prepared product manifest")?;
    fs::write(&manifest_path, bytes).with_context(|| {
        format!(
            "writing prepared product manifest '{}'",
            manifest_path.display()
        )
    })
}

fn relative_prepared_product_path(output_dir: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(output_dir).with_context(|| {
        format!(
            "prepared product path '{}' is not under output dir '{}'",
            path.display(),
            output_dir.display()
        )
    })?;
    Ok(relative.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_prepared_output_names_are_product_native() {
        let product = crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_BOOT))
            .expect("parse live-boot");
        let names = canonical_prepared_output_names(product);
        assert_eq!(
            names.rootfs_source_pointer_filename,
            ".live-rootfs-source.path"
        );
        assert_eq!(names.rootfs_erofs_filename, "filesystem.erofs");
        assert_eq!(names.overlay_erofs_filename, "overlayfs.erofs");
    }

    #[test]
    fn compatibility_prepared_output_names_preserve_stage_artifacts() {
        let stage = crate::workflows::parse_stage(Some("01Boot")).expect("parse stage");
        let names = compatibility_prepared_output_names(stage);
        assert_eq!(
            names.rootfs_source_pointer_filename,
            ".s01-live-rootfs-source.path"
        );
        assert_eq!(names.rootfs_erofs_filename, "s01-filesystem.erofs");
        assert_eq!(names.overlay_erofs_filename, "s01-overlayfs.erofs");
    }
}
