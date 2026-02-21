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

use crate::{BuildStage, StageOutputLayout};

pub(crate) fn preflight_iso_build(
    repo_root: &Path,
    distro_id: &str,
    stage: BuildStage,
) -> Result<()> {
    if stage.slug == crate::STAGE00_SLUG {
        return Ok(());
    }

    if stage.slug == crate::STAGE01_SLUG {
        let s00_root =
            crate::stage_paths::stage_output_dir_for(repo_root, distro_id, crate::STAGE00_DIRNAME);
        let run_id =
            crate::stage_runs::latest_successful_stage_run_id(&s00_root)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "preflight failed for '{}' {}: no successful Stage 00 runs found under '{}'.\n\
                     Build Stage 00 first: `just build 0 {}`",
                    distro_id,
                    stage.canonical,
                    s00_root.display(),
                    distro_id
                )
            })?;
        let parent_rootfs = s00_root.join(&run_id).join("s00-filesystem.erofs");
        if !parent_rootfs.is_file() {
            bail!(
                "preflight failed for '{}' {}: missing Stage 00 rootfs image '{}'.\n\
                 Build Stage 00 first: `just build 0 {}`",
                distro_id,
                stage.canonical,
                parent_rootfs.display(),
                distro_id
            );
        }
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

pub(crate) fn build_all(stage: BuildStage) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let distro_ids = crate::workflows::parse::discover_distro_ids(&cwd)?;
    for distro_id in &distro_ids {
        println!("[iso:{}] building {}...", stage.slug, distro_id);
        build_one(distro_id, stage)?;
    }
    Ok(())
}

pub(crate) fn build_one(distro_id: &str, stage: BuildStage) -> Result<()> {
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
    let stage_layout = stage_output_layout_for(&bundle.repo_root, distro_id, stage)?;

    let stage_output_dir = stage_layout.stage_output_dir.clone();

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
        kernel_version: bundle.contract.stages.stage_00_build.kernel_version.clone(),
        kernel_sha256: bundle.contract.stages.stage_00_build.kernel_sha256.clone(),
        kernel_localversion: bundle
            .contract
            .stages
            .stage_00_build
            .kernel_localversion
            .clone(),
        module_install_path: bundle
            .contract
            .stages
            .stage_00_build
            .module_install_path
            .clone(),
    };

    let created_at_utc = now_utc_compact()?;
    let iso_path = stage_output_dir.join(iso_filename_for_stage(
        &bundle.contract.artifacts.iso_filename,
        stage,
    ));

    if let Some(run_id) = stage_layout.run_id.as_deref() {
        let metadata_path = crate::stage_runs::manifest_path(&stage_output_dir);
        crate::stage_run_manifest::write_stage_run_metadata(
            &metadata_path,
            &crate::StageRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                stage_name: stage.canonical.to_string(),
                stage_slug: stage.slug.to_string(),
                status: "building".to_string(),
                created_at_utc: created_at_utc.clone(),
                finished_at_utc: None,
                stage_root_dir: stage_layout.stage_root_dir.display().to_string(),
                stage_output_dir: stage_output_dir.display().to_string(),
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
                println!("[iso:{}:{distro_id}] kernel already installed", stage.slug);
            }
        }
        ensure_iso_exists(&bundle, distro_id, &kernel_output_dir, &stage_layout, stage)?;

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
            iso_filename: iso_filename_for_stage(&bundle.contract.artifacts.iso_filename, stage),
        };

        run_00build_evidence_script(
            &bundle.repo_root,
            &bundle.variant_dir,
            &kernel_output_dir,
            &stage_output_dir,
            &evidence_spec,
        )
        .with_context(|| format!("running 00Build evidence for '{distro_id}'"))?;

        println!(
            "[iso:{}:{distro_id}] stage {} passed; ISO at {}",
            stage.slug,
            stage.canonical,
            stage_output_dir
                .join(iso_filename_for_stage(
                    &bundle.contract.artifacts.iso_filename,
                    stage
                ))
                .display()
        );
        Ok(())
    })();

    if let Some(run_id) = stage_layout.run_id.as_deref() {
        let metadata_path = crate::stage_runs::manifest_path(&stage_output_dir);
        let finished_at_utc = Some(now_utc_compact()?);
        let status = if build_result.is_ok() {
            "success".to_string()
        } else {
            "failed".to_string()
        };
        let metadata_result = crate::stage_run_manifest::write_stage_run_metadata(
            &metadata_path,
            &crate::StageRunMetadata {
                run_id: run_id.to_string(),
                distro_id: distro_id.to_string(),
                stage_name: stage.canonical.to_string(),
                stage_slug: stage.slug.to_string(),
                status,
                created_at_utc,
                finished_at_utc,
                stage_root_dir: stage_layout.stage_root_dir.display().to_string(),
                stage_output_dir: stage_output_dir.display().to_string(),
                iso_path: iso_path.display().to_string(),
            },
        );
        if let Err(err) = metadata_result {
            if build_result.is_ok() {
                return Err(err);
            }
            eprintln!(
                "[iso:{}:{distro_id}] warning: failed to persist stage run metadata: {err:#}",
                stage.slug
            );
        }

        if build_result.is_ok() {
            crate::stage_runs::prune_old_stage_runs(
                &stage_layout.stage_root_dir,
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
    stage_layout: &StageOutputLayout,
    stage: BuildStage,
) -> Result<()> {
    let stage_output_dir = &stage_layout.stage_output_dir;
    let iso_filename = iso_filename_for_stage(&bundle.contract.artifacts.iso_filename, stage);
    let iso_path = stage_output_dir.join(&iso_filename);
    let native_build = bundle.variant_dir.join(native_build_script_filename(stage));
    if !native_build.is_file() {
        bail!(
            "missing variant-native {} build hook for '{}': {}\n\
             legacy crate entrypoints are blocked by policy.\n\
             add '{}' under {} and implement ISO assembly there.",
            stage.canonical,
            distro_id,
            native_build.display(),
            native_build_script_filename(stage),
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
        "[iso:{}:{distro_id}] building run {} via {} (output: {})",
        stage.slug,
        stage_layout.run_id.as_deref().unwrap_or("adhoc"),
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
        .env("BUILD_STAGE_NAME", stage.canonical)
        .env("BUILD_STAGE_SLUG", stage.slug)
        .env("BUILD_STAGE_DIRNAME", stage.dir_name)
        .env("STAGE_ROOT_DIR", &stage_layout.stage_root_dir)
        .env("STAGE_RUN_DIR", stage_output_dir)
        .env(
            "STAGE_REQUIRED_KERNEL_CMDLINE",
            stage_required_kernel_cmdline(bundle, stage),
        )
        .env("KERNEL_OUTPUT_DIR", kernel_output_dir)
        .env("STAGE_OUTPUT_DIR", stage_output_dir)
        .env("BUILD_RUN_ID", stage_layout.run_id.as_deref().unwrap_or(""))
        .env("DISTRO_BUILDER_BIN", &distro_builder_bin)
        .status()
        .with_context(|| {
            format!(
                "running {} native build hook for '{}' using {}",
                stage.canonical,
                distro_id,
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

fn stage_required_kernel_cmdline(bundle: &LoadedVariantContract, stage: BuildStage) -> String {
    match stage.slug {
        crate::STAGE01_SLUG | crate::STAGE02_SLUG => bundle
            .contract
            .stages
            .stage_01_live_boot
            .required_kernel_cmdline
            .join(" "),
        _ => String::new(),
    }
}

fn native_build_script_filename(stage: BuildStage) -> &'static str {
    match stage.slug {
        crate::STAGE00_SLUG => crate::STAGE00_NATIVE_BUILD_SCRIPT,
        crate::STAGE01_SLUG => crate::STAGE01_NATIVE_BUILD_SCRIPT,
        crate::STAGE02_SLUG => crate::STAGE02_NATIVE_BUILD_SCRIPT,
        _ => unreachable!("validated in parse_stage"),
    }
}

pub(crate) fn iso_filename_for_stage(stage00_iso_filename: &str, stage: BuildStage) -> String {
    match stage.slug {
        crate::STAGE00_SLUG => stage00_iso_filename.to_string(),
        crate::STAGE01_SLUG | crate::STAGE02_SLUG => {
            derive_stage_iso_filename(stage00_iso_filename, stage.slug)
        }
        _ => unreachable!("validated in parse_stage"),
    }
}

pub(crate) fn derive_stage_iso_filename(stage00_iso_filename: &str, stage_slug: &str) -> String {
    if stage00_iso_filename.contains(crate::STAGE00_SLUG) {
        return stage00_iso_filename.replacen(crate::STAGE00_SLUG, stage_slug, 1);
    }
    if let Some(base) = stage00_iso_filename.strip_suffix(".iso") {
        return format!("{base}-{stage_slug}.iso");
    }
    format!("{stage00_iso_filename}-{stage_slug}.iso")
}

pub(crate) fn stage_output_layout_for(
    repo_root: &Path,
    distro_id: &str,
    stage: BuildStage,
) -> Result<StageOutputLayout> {
    let stage_root_dir =
        crate::stage_paths::stage_output_dir_for(repo_root, distro_id, stage.dir_name);
    std::fs::create_dir_all(&stage_root_dir).with_context(|| {
        format!(
            "creating stage output root directory '{}'",
            stage_root_dir.display()
        )
    })?;
    let (run_id, run_root) = crate::stage_runs::allocate_stage_run_dir(&stage_root_dir)?;

    Ok(StageOutputLayout {
        stage_root_dir,
        stage_output_dir: run_root,
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
