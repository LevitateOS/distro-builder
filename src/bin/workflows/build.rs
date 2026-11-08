use anyhow::{Context, Result};
use std::path::Path;
use time::OffsetDateTime;

use distro_builder::product::{ProductId, ReleaseProduct};

use crate::BuildOutputLayout;

/// Build the releases `product` is built on that have no rootfs yet.
pub(crate) fn ensure_release_prerequisites(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
) -> Result<()> {
    let variant = distro_contract::load(repo_root, distro_id)?;
    let missing =
        distro_builder::missing_parent_releases(repo_root, distro_id, &variant.contract, product)?;
    for parent in missing {
        let release = ReleaseProduct::new(parent)
            .with_context(|| format!("{product} is built on {parent}, which has no release"))?;
        println!(
            "[release:iso:{product}:{distro_id}] materializing missing parent release '{parent}'..."
        );
        build_one(distro_id, release)?;
    }

    Ok(())
}

pub(crate) fn build_all(release: ReleaseProduct) -> Result<()> {
    let product = release.product;
    let repo_root = crate::workflows::repo_root()?;
    let distro_ids = crate::workflows::parse::discover_distro_ids(&repo_root)?;
    for distro_id in &distro_ids {
        println!("[release:iso:{product}] building {distro_id}...");
        ensure_release_prerequisites(&repo_root, distro_id, product)?;
        build_one(distro_id, release)?;
    }
    Ok(())
}

pub(crate) fn build_one(distro_id: &str, release: ReleaseProduct) -> Result<()> {
    let product = release.product;
    let repo_root = crate::workflows::repo_root()?;
    let bundle = distro_contract::load(&repo_root, distro_id)?;

    let kernel_output_dir =
        distro_builder::run_history::kernel_output_dir(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let build_layout = product_release_output_layout_for(&bundle.repo_root, distro_id, product)?;

    let output_dir = build_layout.output_dir.clone();
    let created_at_utc = now_utc_compact()?;
    let iso_path = output_dir.join(bundle.contract.artifacts.iso(product));

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = distro_builder::run_history::run_manifest_path(&output_dir);
        crate::run_manifest::write_run_metadata(
            &metadata_path,
            &crate::BuildRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                target_kind: distro_builder::run_history::TARGET_KIND_RELEASE_PRODUCT.to_string(),
                target_name: product.name().to_string(),
                status: distro_builder::run_history::STATUS_BUILDING.to_string(),
                created_at_utc: created_at_utc.clone(),
                finished_at_utc: None,
                root_dir: build_layout.root_dir.display().to_string(),
                output_dir: output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        )?;
    }

    let build_result = (|| -> Result<()> {
        distro_builder::kernel::ensure_kernel_preinstalled(&bundle, &kernel_output_dir)
            .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?;
        println!("[release:iso:{product}:{distro_id}] kernel already installed");
        crate::workflows::ensure_release_iso_via_variant_hook(
            &bundle,
            distro_id,
            &kernel_output_dir,
            &build_layout,
            release,
        )?;

        println!(
            "[release:iso:{product}:{distro_id}] built at {}",
            iso_path.display()
        );
        Ok(())
    })();

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = distro_builder::run_history::run_manifest_path(&output_dir);
        let finished_at_utc = Some(now_utc_compact()?);
        let status = if build_result.is_ok() {
            distro_builder::run_history::STATUS_SUCCESS.to_string()
        } else {
            distro_builder::run_history::STATUS_FAILED.to_string()
        };
        let metadata_result = crate::run_manifest::write_run_metadata(
            &metadata_path,
            &crate::BuildRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                target_kind: distro_builder::run_history::TARGET_KIND_RELEASE_PRODUCT.to_string(),
                target_name: product.name().to_string(),
                status,
                created_at_utc,
                finished_at_utc,
                root_dir: build_layout.root_dir.display().to_string(),
                output_dir: output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        );
        if let Err(err) = metadata_result {
            if build_result.is_ok() {
                return Err(err);
            }
            eprintln!(
                "[release:iso:{product}:{distro_id}] warning: failed to persist release run metadata: {err:#}"
            );
        }

        if build_result.is_ok() {
            distro_builder::run_history::prune_old_runs(
                &build_layout.root_dir,
                crate::RELEASE_RUN_RETENTION_COUNT,
            )?;
        }
    }

    build_result
}

pub(crate) fn product_release_output_layout_for(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
) -> Result<BuildOutputLayout> {
    let root_dir = distro_builder::run_history::release_product_dir(repo_root, distro_id, product);
    std::fs::create_dir_all(&root_dir).with_context(|| {
        format!(
            "creating product release root directory '{}'",
            root_dir.display()
        )
    })?;
    let (run_id, run_root) = distro_builder::run_history::allocate_run_dir(&root_dir)?;

    Ok(BuildOutputLayout {
        root_dir,
        output_dir: run_root,
        run_id: Some(run_id),
    })
}

pub(crate) fn now_utc_compact() -> Result<String> {
    let now = OffsetDateTime::now_utc();
    Ok(format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        now.month() as u8,
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    ))
}
