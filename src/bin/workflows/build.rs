use anyhow::{bail, Context, Result};
use distro_builder::stages::s00_build::{
    ensure_kernel_installed_via_recipe, run_00build_evidence_script, S00BuildEvidenceSpec,
    S00BuildKernelEnsureOutcome, S00BuildKernelSpec,
};
use distro_contract::{load_stage_00_contract_bundle_for_distro_from, require_valid_contract};
use std::path::Path;
use std::process::Command;
use time::OffsetDateTime;

use crate::{BuildOutputLayout, BuildProduct};

pub(crate) fn preflight_iso_build(
    repo_root: &Path,
    distro_id: &str,
    product: BuildProduct,
) -> Result<()> {
    let bundle = load_stage_00_contract_bundle_for_distro_from(repo_root, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;
    if let Some(parent_product) = parent_product_for_release_product(&bundle.contract, product)? {
        require_parent_product_rootfs(repo_root, distro_id, product, parent_product)?;
    }

    Ok(())
}

fn parent_product_for_release_product(
    contract: &distro_contract::ConformanceContract,
    product: BuildProduct,
) -> Result<Option<BuildProduct>> {
    let parent_logical_name = match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => None,
        crate::PRODUCT_LIVE_BOOT => contract.products.boot_live.extends.as_deref(),
        crate::PRODUCT_LIVE_TOOLS => contract.products.live_tools.extends.as_deref(),
        _ => unreachable!("validated in parse_product"),
    };

    parent_logical_name
        .map(crate::workflows::product_for_logical_name)
        .transpose()
}

fn require_parent_product_rootfs(
    repo_root: &Path,
    distro_id: &str,
    product: BuildProduct,
    parent_product: BuildProduct,
) -> Result<()> {
    let parent_root = crate::stage_paths::release_product_dir_for(
        repo_root,
        distro_id,
        parent_product.release_dir_name,
    );
    let run_id = crate::stage_runs::latest_successful_run_id(&parent_root)?.ok_or_else(|| {
        anyhow::anyhow!(
            "preflight failed for '{}' release product '{}': no successful parent product '{}' runs found under '{}'.\n\
             Build the '{}' release first: `cargo run -p distro-builder --bin distro-builder -- release build iso {} {}`",
            distro_id,
            product.canonical,
            parent_product.canonical,
            parent_root.display(),
            parent_product.canonical,
            distro_id,
            parent_product.canonical
        )
    })?;
    let parent_rootfs = parent_root
        .join(&run_id)
        .join(parent_product.rootfs_erofs_filename);
    if !parent_rootfs.is_file() {
        bail!(
            "preflight failed for '{}' release product '{}': missing parent product '{}' rootfs image '{}'.\n\
             Build the '{}' release first: `cargo run -p distro-builder --bin distro-builder -- release build iso {} {}`",
            distro_id,
            product.canonical,
            parent_product.canonical,
            parent_rootfs.display(),
            parent_product.canonical,
            distro_id,
            parent_product.canonical
        );
    }
    Ok(())
}

pub(crate) fn enforce_legacy_binding_policy_guard() -> Result<()> {
    let repo_root = crate::workflows::layout::locate_repo_root()?;
    let status = Command::new("cargo")
        .current_dir(&repo_root)
        .args(["xtask", "policy", "audit-legacy-bindings"])
        .status()
        .context("running legacy-binding policy guard via `cargo xtask`")?;

    if status.success() {
        return Ok(());
    }

    bail!(
        "policy guard failed before distro-builder execution (exit: {}). \
Run `cargo xtask policy audit-legacy-bindings` and fix violations first.",
        status
    )
}

pub(crate) fn build_all(product: BuildProduct) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let distro_ids = crate::workflows::parse::discover_distro_ids(&cwd)?;
    for distro_id in &distro_ids {
        println!(
            "[release:iso:{}] building {}...",
            product.canonical, distro_id
        );
        build_one(distro_id, product)?;
    }
    Ok(())
}

pub(crate) fn build_one(distro_id: &str, product: BuildProduct) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{distro_id}'"))?;

    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating 00Build contract for '{distro_id}'"))?;

    let kernel_output_dir = crate::stage_paths::kernel_output_dir_for(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let build_layout = product_release_output_layout_for(&bundle.repo_root, distro_id, product)?;

    let output_dir = build_layout.output_dir.clone();

    let kernel_spec = S00BuildKernelSpec {
        recipe_kernel_script: bundle
            .contract
            .stages
            .stage_00_build
            .recipe_kernel_script
            .clone(),
        kernel_kconfig_path: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_kconfig_path
            .clone(),
    };

    let created_at_utc = now_utc_compact()?;
    let iso_path = output_dir.join(iso_filename_for_product(
        &bundle.contract.artifacts.iso_filename,
        product,
    ));

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = crate::stage_runs::run_manifest_path(&output_dir);
        crate::stage_run_manifest::write_run_metadata(
            &metadata_path,
            &crate::BuildRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                target_kind: "release-product".to_string(),
                target_name: product.canonical.to_string(),
                status: "building".to_string(),
                created_at_utc: created_at_utc.clone(),
                finished_at_utc: None,
                root_dir: build_layout.root_dir.display().to_string(),
                output_dir: output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        )?;
    }

    let build_result = (|| -> Result<()> {
        match ensure_kernel_installed_via_recipe(
            &bundle.repo_root,
            &bundle.variant_dir,
            distro_id,
            &kernel_output_dir,
            &kernel_spec,
        )
        .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?
        {
            S00BuildKernelEnsureOutcome::AlreadyInstalled => {
                println!(
                    "[release:iso:{}:{distro_id}] kernel already installed",
                    product.canonical
                );
            }
        }
        crate::workflows::ensure_release_iso_via_compatibility_hook(
            &bundle,
            distro_id,
            &kernel_output_dir,
            &build_layout,
            product,
        )?;

        let evidence_spec = S00BuildEvidenceSpec {
            script_path: bundle
                .contract
                .stages
                .stage_00_build
                .evidence
                .script_path
                .clone(),
            pass_marker: bundle
                .contract
                .stages
                .stage_00_build
                .evidence
                .pass_marker
                .clone(),
            kernel_release_path: bundle
                .contract
                .stages
                .stage_00_build
                .kernel_release_path
                .clone(),
            kernel_image_path: bundle
                .contract
                .stages
                .stage_00_build
                .kernel_image_path
                .clone(),
            iso_filename: iso_filename_for_product(
                &bundle.contract.artifacts.iso_filename,
                product,
            ),
        };

        run_00build_evidence_script(
            &bundle.repo_root,
            &bundle.variant_dir,
            &kernel_output_dir,
            &output_dir,
            &evidence_spec,
        )
        .with_context(|| format!("running 00Build evidence for '{distro_id}'"))?;

        println!(
            "[release:iso:{}:{distro_id}] built at {}",
            product.canonical,
            output_dir
                .join(iso_filename_for_product(
                    &bundle.contract.artifacts.iso_filename,
                    product
                ))
                .display()
        );
        Ok(())
    })();

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = crate::stage_runs::run_manifest_path(&output_dir);
        let finished_at_utc = Some(now_utc_compact()?);
        let status = if build_result.is_ok() {
            "success".to_string()
        } else {
            "failed".to_string()
        };
        let metadata_result = crate::stage_run_manifest::write_run_metadata(
            &metadata_path,
            &crate::BuildRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                target_kind: "release-product".to_string(),
                target_name: product.canonical.to_string(),
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
                "[release:iso:{}:{distro_id}] warning: failed to persist stage run metadata: {err:#}",
                product.canonical
            );
        }

        if build_result.is_ok() {
            crate::stage_runs::prune_old_runs(
                &build_layout.root_dir,
                crate::S00_RUN_RETENTION_COUNT,
            )?;
        }
    }

    build_result
}

pub(crate) fn iso_filename_for_product(base_iso_filename: &str, product: BuildProduct) -> String {
    match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => base_iso_filename.to_string(),
        crate::PRODUCT_LIVE_BOOT | crate::PRODUCT_LIVE_TOOLS => {
            derive_product_iso_filename(base_iso_filename, product.iso_suffix)
        }
        _ => unreachable!("validated in parse_product"),
    }
}

pub(crate) fn derive_product_iso_filename(base_iso_filename: &str, product_suffix: &str) -> String {
    if let Some(base) = base_iso_filename.strip_suffix(".iso") {
        return format!("{base}-{product_suffix}.iso");
    }
    format!("{base_iso_filename}-{product_suffix}.iso")
}

pub(crate) fn product_release_output_layout_for(
    repo_root: &Path,
    distro_id: &str,
    product: BuildProduct,
) -> Result<BuildOutputLayout> {
    let root_dir =
        crate::stage_paths::release_product_dir_for(repo_root, distro_id, product.release_dir_name);
    std::fs::create_dir_all(&root_dir).with_context(|| {
        format!(
            "creating product release root directory '{}'",
            root_dir.display()
        )
    })?;
    let (run_id, run_root) = crate::stage_runs::allocate_run_dir(&root_dir)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn product_iso_filename_is_product_native() {
        let product = crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_BOOT))
            .expect("parse live-boot");
        assert_eq!(
            derive_product_iso_filename("levitateos-x86_64.iso", product.iso_suffix),
            "levitateos-x86_64-live-boot.iso"
        );
    }

    #[test]
    fn base_rootfs_iso_filename_stays_base_name() {
        let product = crate::workflows::parse_product(Some(crate::PRODUCT_BASE_ROOTFS))
            .expect("parse base-rootfs");
        assert_eq!(
            iso_filename_for_product("levitateos-x86_64.iso", product),
            "levitateos-x86_64.iso"
        );
    }

    #[test]
    fn product_release_output_layout_uses_release_root() {
        let repo_root = tempfile::tempdir().expect("repo tempdir");
        let product = crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_TOOLS))
            .expect("parse live-tools");
        let layout = product_release_output_layout_for(repo_root.path(), "levitate", product)
            .expect("allocate product release layout");
        assert!(
            layout.root_dir.ends_with("levitate/releases/live-tools"),
            "unexpected release root '{}'",
            layout.root_dir.display()
        );
        assert!(
            layout.output_dir.starts_with(&layout.root_dir),
            "run dir '{}' must live under release root '{}'",
            layout.output_dir.display(),
            layout.root_dir.display()
        );
    }
}
