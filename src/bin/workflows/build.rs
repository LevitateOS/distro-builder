use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use distro_builder::stages::s00_build::{
    ensure_kernel_installed_via_recipe, run_00build_evidence_script, S00BuildEvidenceSpec,
    S00BuildKernelEnsureOutcome, S00BuildKernelSpec,
};
use distro_contract::{
    load_stage_00_contract_bundle_for_distro_from, require_valid_contract, LoadedVariantContract,
};
use time::OffsetDateTime;

use crate::{BuildOutputLayout, BuildProduct, CompatibilityBuildStage};

pub(crate) fn preflight_iso_build(
    repo_root: &Path,
    distro_id: &str,
    product: BuildProduct,
) -> Result<()> {
    match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => {}
        crate::PRODUCT_LIVE_BOOT => require_parent_product_rootfs(
            repo_root,
            distro_id,
            product.canonical,
            crate::PRODUCT_BASE_ROOTFS,
            "filesystem.erofs",
        )?,
        crate::PRODUCT_LIVE_TOOLS => require_parent_product_rootfs(
            repo_root,
            distro_id,
            product.canonical,
            crate::PRODUCT_LIVE_BOOT,
            "filesystem.erofs",
        )?,
        _ => unreachable!("validated in parse_stage"),
    }

    Ok(())
}

fn require_parent_product_rootfs(
    repo_root: &Path,
    distro_id: &str,
    product_name: &str,
    parent_product_name: &str,
    parent_rootfs_filename: &str,
) -> Result<()> {
    let parent_root =
        crate::stage_paths::release_product_dir_for(repo_root, distro_id, parent_product_name);
    let run_id = crate::stage_runs::latest_successful_run_id(&parent_root)?.ok_or_else(|| {
        anyhow::anyhow!(
            "preflight failed for '{}' release product '{}': no successful parent product '{}' runs found under '{}'.\n\
             Build the '{}' release first: `cargo run -p distro-builder --bin distro-builder -- release build iso {} {}`",
            distro_id,
            product_name,
            parent_product_name,
            parent_root.display(),
            parent_product_name,
            distro_id,
            parent_product_name
        )
    })?;
    let parent_rootfs = parent_root.join(&run_id).join(parent_rootfs_filename);
    if !parent_rootfs.is_file() {
        bail!(
            "preflight failed for '{}' release product '{}': missing parent product '{}' rootfs image '{}'.\n\
             Build the '{}' release first: `cargo run -p distro-builder --bin distro-builder -- release build iso {} {}`",
            distro_id,
            product_name,
            parent_product_name,
            parent_rootfs.display(),
            parent_product_name,
            distro_id,
            parent_product_name
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
    let compat_stage = crate::workflows::compatibility_stage_for_product(product);
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
                compatibility_stage_name: compat_stage.canonical.to_string(),
                compatibility_stage_slug: compat_stage.slug.to_string(),
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
        ensure_iso_exists(
            &bundle,
            distro_id,
            &kernel_output_dir,
            &build_layout,
            product,
            compat_stage,
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
            "[release:iso:{}:{distro_id}] built via compatibility stage {} at {}",
            product.canonical,
            compat_stage.canonical,
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
                compatibility_stage_name: compat_stage.canonical.to_string(),
                compatibility_stage_slug: compat_stage.slug.to_string(),
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

fn ensure_iso_exists(
    bundle: &LoadedVariantContract,
    distro_id: &str,
    kernel_output_dir: &Path,
    build_layout: &BuildOutputLayout,
    product: BuildProduct,
    compat_stage: CompatibilityBuildStage,
) -> Result<()> {
    let output_dir = &build_layout.output_dir;
    let iso_filename = iso_filename_for_product(&bundle.contract.artifacts.iso_filename, product);
    let iso_path = output_dir.join(&iso_filename);
    let native_build = bundle.variant_dir.join(compat_stage.native_build_script);
    if !native_build.is_file() {
        bail!(
            "missing variant-native {} build hook for '{}': {}\n\
             legacy crate entrypoints are blocked by policy.\n\
             add '{}' under {} and implement ISO assembly there.",
            compat_stage.canonical,
            distro_id,
            native_build.display(),
            compat_stage.native_build_script,
            bundle.variant_dir.display()
        );
    }

    let kernel_release_path =
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_release_path);
    let kernel_image_path =
        kernel_output_dir.join(&bundle.contract.stages.stage_00_build.kernel_image_path);

    // Builds always target a freshly allocated per-run output directory.
    // "missing ISO" in that directory is expected and not a cache miss.
    println!(
        "[release:iso:{}:{distro_id}] building release run {} via {} (output: {})",
        product.canonical,
        build_layout.run_id.as_deref().unwrap_or("adhoc"),
        native_build.display(),
        iso_path.display()
    );

    let distro_builder_bin =
        std::env::current_exe().context("resolving distro-builder executable path")?;

    let status = Command::new("sh")
        .arg(&native_build)
        .current_dir(&bundle.repo_root)
        .env("DISTRO_ID", distro_id)
        .env("IDENTITY_OS_NAME", &bundle.contract.identity.os_name)
        .env("IDENTITY_OS_ID", &bundle.contract.identity.os_id)
        .env("IDENTITY_OS_VERSION", &bundle.contract.identity.os_version)
        .env("IDENTITY_ISO_LABEL", &bundle.contract.identity.iso_label)
        .env(
            "S00_LIVE_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_uki_filename,
        )
        .env(
            "S00_EMERGENCY_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .emergency_uki_filename,
        )
        .env(
            "S00_DEBUG_UKI_FILENAME",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .debug_uki_filename,
        )
        .env(
            "S00_LIVE_CMDLINE",
            &bundle
                .contract
                .stages
                .stage_00_build
                .iso_assembly
                .live_cmdline,
        )
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .env("ISO_FILENAME", &iso_filename)
        .env("PRODUCT_NAME", product.canonical)
        .env("BUILD_TARGET_LABEL", product.issue_banner_label)
        .env("ROOTFS_FILENAME", product.rootfs_erofs_filename)
        .env("INITRAMFS_LIVE_FILENAME", product.initramfs_live_filename)
        .env("LIVE_OVERLAY_DIRNAME", product.live_overlay_dir_name)
        .env(
            "LIVE_OVERLAY_IMAGE_FILENAME",
            product.overlay_erofs_filename,
        )
        .env(
            "ROOTFS_SOURCE_POINTER_FILENAME",
            product.rootfs_source_pointer_filename,
        )
        .env("BUILD_STAGE_NAME", compat_stage.canonical)
        .env("BUILD_STAGE_SLUG", compat_stage.slug)
        .env("BUILD_STAGE_DIRNAME", compat_stage.dir_name)
        .env("STAGE_ARTIFACT_TAG", compat_stage.artifact_tag)
        .env("STAGE_ROOT_DIR", &build_layout.root_dir)
        .env("STAGE_RUN_DIR", output_dir)
        .env(
            "STAGE_REQUIRED_KERNEL_CMDLINE",
            stage_required_kernel_cmdline(bundle, compat_stage),
        )
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("STAGE_OUTPUT_DIR", output_dir)
        .env("BUILD_RUN_ID", build_layout.run_id.as_deref().unwrap_or(""))
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .status()
        .with_context(|| {
            format!(
                "running {} native build hook for '{}' from release product '{}' using {}",
                compat_stage.canonical,
                distro_id,
                product.canonical,
                native_build.display()
            )
        })?;

    if !status.success() {
        bail!("builder command failed for '{distro_id}' with status {status}");
    }

    if !iso_path.is_file() {
        bail!(
            "builder finished but ISO still missing for '{}': {}",
            distro_id,
            iso_path.display()
        );
    }

    Ok(())
}

fn stage_required_kernel_cmdline(
    bundle: &LoadedVariantContract,
    compat_stage: CompatibilityBuildStage,
) -> String {
    match compat_stage.slug {
        "s01_boot" | "s02_live_tools" => bundle
            .contract
            .stages
            .stage_01_live_boot
            .required_kernel_cmdline
            .join(" "),
        _ => String::new(),
    }
}

pub(crate) fn iso_filename_for_product(
    stage00_iso_filename: &str,
    product: BuildProduct,
) -> String {
    match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => {
            derive_product_iso_filename(stage00_iso_filename, product.iso_suffix)
        }
        crate::PRODUCT_LIVE_BOOT | crate::PRODUCT_LIVE_TOOLS => {
            derive_product_iso_filename(stage00_iso_filename, product.iso_suffix)
        }
        _ => unreachable!("validated in parse_product"),
    }
}

pub(crate) fn derive_product_iso_filename(
    stage00_iso_filename: &str,
    product_suffix: &str,
) -> String {
    if stage00_iso_filename.contains("s00_build") {
        return stage00_iso_filename.replacen("s00_build", product_suffix, 1);
    }
    if stage00_iso_filename.contains("s00-build") {
        return stage00_iso_filename.replacen("s00-build", product_suffix, 1);
    }
    if let Some(base) = stage00_iso_filename.strip_suffix(".iso") {
        return format!("{base}-{product_suffix}.iso");
    }
    format!("{stage00_iso_filename}-{product_suffix}.iso")
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
            derive_product_iso_filename("levitateos-x86_64-s00_build.iso", product.iso_suffix),
            "levitateos-x86_64-live-boot.iso"
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
