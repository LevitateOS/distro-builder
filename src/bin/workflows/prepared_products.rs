use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use distro_builder::product::ProductId;
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

/// The prepared product's file names: the table's pointer file, the
/// contract's image names.
pub(crate) fn prepared_output_names(
    contract: &distro_contract::ConformanceContract,
    product: ProductId,
) -> PreparedOutputNames {
    PreparedOutputNames {
        rootfs_source_pointer_filename: distro_builder::product::build(product)
            .rootfs_source_pointer
            .to_string(),
        rootfs_erofs_filename: contract.artifacts.rootfs.clone(),
        overlay_erofs_filename: contract.artifacts.overlay.clone(),
    }
}

pub(crate) fn write_prepared_product_outputs(
    output_dir: &Path,
    product: ProductId,
    distro_id: &str,
    prepared: &PreparedProductInputs,
    output_names: &PreparedOutputNames,
) -> Result<PathBuf> {
    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file = output_dir.join(&output_names.rootfs_source_pointer_filename);
    fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing product '{}' rootfs source path file '{}'",
            product,
            source_path_file.display()
        )
    })?;
    write_prepared_product_manifest(
        output_dir,
        &PreparedProductManifest {
            product: product.name().to_string(),
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
