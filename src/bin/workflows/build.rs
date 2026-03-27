use anyhow::{bail, Context, Result};
use distro_builder::build_host::{
    ensure_kernel_preinstalled_via_recipe, run_build_host_evidence_script, BuildHostEvidenceSpec,
    BuildHostKernelEnsureOutcome, BuildHostKernelSpec,
};
use distro_contract::{load_variant_contract_bundle_for_distro_from, require_valid_contract};
use std::path::Path;
use std::process::Command;
use time::OffsetDateTime;

use crate::{BuildOutputLayout, BuildProduct};

pub(crate) fn ensure_release_prerequisites(
    repo_root: &Path,
    distro_id: &str,
    product: BuildProduct,
) -> Result<()> {
    let bundle = load_variant_contract_bundle_for_distro_from(repo_root, distro_id)
        .with_context(|| format!("loading variant contract for '{}'", distro_id))?;
    let prerequisite_plan = distro_builder::plan_release_prerequisite_realization(
        repo_root,
        distro_id,
        &bundle.contract,
        product.canonical,
    )?;
    for prerequisite in prerequisite_plan.missing_products() {
        let prerequisite = crate::workflows::parse_product(Some(prerequisite))?;
        println!(
            "[release:iso:{}:{distro_id}] materializing missing parent release '{}'...",
            product.canonical, prerequisite.canonical
        );
        build_one(distro_id, prerequisite)?;
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
        ensure_release_prerequisites(&cwd, distro_id, product)?;
        build_one(distro_id, product)?;
    }
    Ok(())
}

pub(crate) fn build_one(distro_id: &str, product: BuildProduct) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_variant_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading variant contract for '{distro_id}'"))?;

    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating variant contract for '{distro_id}'"))?;

    let kernel_output_dir =
        crate::artifact_paths::kernel_output_dir_for(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let build_layout = product_release_output_layout_for(&bundle.repo_root, distro_id, product)?;

    let output_dir = build_layout.output_dir.clone();
    let build = &bundle.contract.build;

    let kernel_spec = BuildHostKernelSpec {
        recipe_kernel_script: build.kernel.recipe_script.clone(),
        kernel_kconfig_path: build.kernel.kconfig_path.clone(),
    };

    let base_iso_filename = crate::workflows::canonical_iso_filename(&bundle.contract)
        .with_context(|| format!("resolving canonical Ring 0 ISO output for '{}'", distro_id))?;
    let created_at_utc = now_utc_compact()?;
    let iso_path = output_dir.join(iso_filename_for_product(&base_iso_filename, product));

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = crate::run_history::run_manifest_path(&output_dir);
        crate::run_manifest::write_run_metadata(
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
        match ensure_kernel_preinstalled_via_recipe(
            &bundle.repo_root,
            &bundle.paths,
            distro_id,
            &kernel_output_dir,
            &kernel_spec,
        )
        .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?
        {
            BuildHostKernelEnsureOutcome::AlreadyInstalled => {
                println!(
                    "[release:iso:{}:{distro_id}] kernel already installed",
                    product.canonical
                );
            }
        }
        crate::workflows::ensure_release_iso_via_variant_hook(
            &bundle,
            distro_id,
            &kernel_output_dir,
            &build_layout,
            product,
        )?;

        let evidence_spec = BuildHostEvidenceSpec {
            script_path: build.evidence.script_path.clone(),
            pass_marker: build.evidence.pass_marker.clone(),
            kernel_release_path: build.kernel.release_path.clone(),
            kernel_image_path: build.kernel.image_path.clone(),
            iso_filename: iso_filename_for_product(&base_iso_filename, product),
        };

        run_build_host_evidence_script(
            &bundle.repo_root,
            &bundle.paths,
            &kernel_output_dir,
            &output_dir,
            &evidence_spec,
        )
        .with_context(|| format!("running build evidence for '{distro_id}'"))?;

        println!(
            "[release:iso:{}:{distro_id}] built at {}",
            product.canonical,
            output_dir
                .join(iso_filename_for_product(&base_iso_filename, product))
                .display()
        );
        Ok(())
    })();

    if let Some(run_id) = build_layout.run_id.as_deref() {
        let metadata_path = crate::run_history::run_manifest_path(&output_dir);
        let finished_at_utc = Some(now_utc_compact()?);
        let status = if build_result.is_ok() {
            "success".to_string()
        } else {
            "failed".to_string()
        };
        let metadata_result = crate::run_manifest::write_run_metadata(
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
                "[release:iso:{}:{distro_id}] warning: failed to persist release run metadata: {err:#}",
                product.canonical
            );
        }

        if build_result.is_ok() {
            crate::run_history::prune_old_runs(
                &build_layout.root_dir,
                crate::RELEASE_RUN_RETENTION_COUNT,
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
    let root_dir = crate::artifact_paths::release_product_dir_for(
        repo_root,
        distro_id,
        product.release_dir_name,
    );
    std::fs::create_dir_all(&root_dir).with_context(|| {
        format!(
            "creating product release root directory '{}'",
            root_dir.display()
        )
    })?;
    let (run_id, run_root) = crate::run_history::allocate_run_dir(&root_dir)?;

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
