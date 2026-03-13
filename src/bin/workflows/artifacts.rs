use std::path::Path;
use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use distro_builder::recipe::alpine_stage01::preseed_alpine_stage01_assets;
use distro_builder::recipe::stage01_source::preseed_stage01_dvd;
use distro_builder::stages::s01_boot_inputs::{
    load_s00_build_input_spec, load_s01_boot_input_spec,
    prepare_s00_build_inputs as prepare_s00_build_inputs_for_distro,
    prepare_s01_boot_inputs as prepare_s01_boot_inputs_for_distro,
};
use distro_builder::stages::s02_live_tools_inputs::{
    load_s02_live_tools_input_spec,
    prepare_s02_live_tools_inputs as prepare_s02_live_tools_inputs_for_distro,
};
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_contract::load_stage_00_contract_bundle_for_distro_from;

pub(crate) fn build_rootfs_erofs(source_dir: &Path, output: &Path) -> Result<()> {
    build_erofs_default(source_dir, output).with_context(|| {
        format!(
            "building rootfs EROFS from '{}' to '{}'",
            source_dir.display(),
            output.display()
        )
    })
}

pub(crate) fn build_overlayfs_erofs(source_dir: &Path, output: &Path) -> Result<()> {
    build_overlayfs_default(source_dir, output).with_context(|| {
        format!(
            "building overlayfs EROFS from '{}' to '{}'",
            source_dir.display(),
            output.display()
        )
    })
}

fn stage_artifact_tag_for_slug(stage_slug: &str) -> &'static str {
    match stage_slug {
        crate::STAGE00_SLUG => crate::STAGE00_ARTIFACT_TAG,
        crate::STAGE01_SLUG => crate::STAGE01_ARTIFACT_TAG,
        crate::STAGE02_SLUG => crate::STAGE02_ARTIFACT_TAG,
        _ => unreachable!("validated in parse_stage"),
    }
}

fn read_stage_rootfs_source_path(path_file: &Path) -> Result<PathBuf> {
    let raw = fs::read_to_string(path_file)
        .with_context(|| format!("reading rootfs source path file '{}'", path_file.display()))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        bail!(
            "rootfs source path file '{}' is empty; expected absolute source directory path",
            path_file.display()
        );
    }
    Ok(PathBuf::from(trimmed))
}

pub(crate) fn build_stage_erofs_cmd(stage: &str, distro_id: &str) -> Result<()> {
    let stage = crate::workflows::parse_stage(Some(stage))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;
    let stage_output_dir =
        crate::stage_paths::stage_output_dir_for(&bundle.repo_root, distro_id, stage.dir_name);

    fs::create_dir_all(&stage_output_dir).with_context(|| {
        format!(
            "creating stage output directory '{}'",
            stage_output_dir.display()
        )
    })?;

    prepare_stage_inputs_cmd(stage.canonical, distro_id, &stage_output_dir).with_context(|| {
        format!(
            "preparing {} inputs for '{}' in '{}'",
            stage.canonical,
            distro_id,
            stage_output_dir.display()
        )
    })?;

    let stage_artifact_tag = stage_artifact_tag_for_slug(stage.slug);
    let rootfs_source_file =
        stage_output_dir.join(format!(".{}-live-rootfs-source.path", stage_artifact_tag));
    let rootfs_source_dir = read_stage_rootfs_source_path(&rootfs_source_file)?;
    let live_overlay_dir = stage_output_dir.join(format!("{stage_artifact_tag}-live-overlay"));
    if !live_overlay_dir.is_dir() {
        bail!(
            "live overlay source directory missing for {} at '{}'\n\
             Remediation: rerun `distro-builder artifact prepare-stage-inputs {} {} {}` and verify overlay preparation succeeds.",
            stage.canonical,
            live_overlay_dir.display(),
            stage.canonical,
            distro_id,
            stage_output_dir.display()
        );
    }

    let rootfs_output = stage_output_dir.join(format!("{stage_artifact_tag}-filesystem.erofs"));
    let overlay_output = stage_output_dir.join(format!("{stage_artifact_tag}-overlayfs.erofs"));

    build_rootfs_erofs(&rootfs_source_dir, &rootfs_output).with_context(|| {
        format!(
            "building {} rootfs EROFS from '{}' to '{}'",
            stage.canonical,
            rootfs_source_dir.display(),
            rootfs_output.display()
        )
    })?;
    build_overlayfs_erofs(&live_overlay_dir, &overlay_output).with_context(|| {
        format!(
            "building {} overlayfs EROFS from '{}' to '{}'",
            stage.canonical,
            live_overlay_dir.display(),
            overlay_output.display()
        )
    })?;

    println!(
        "{} EROFS artifacts built for {}:",
        stage.canonical, distro_id
    );
    println!("  rootfs source: {}", rootfs_source_dir.display());
    println!("  overlay source: {}", live_overlay_dir.display());
    println!("  rootfs erofs: {}", rootfs_output.display());
    println!("  overlay erofs: {}", overlay_output.display());
    Ok(())
}

pub(crate) fn prepare_stage_inputs_cmd(
    stage: &str,
    distro_id: &str,
    output_dir: &Path,
) -> Result<()> {
    let stage = crate::workflows::parse_stage(Some(stage))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    let (prepared_rootfs_source, prepared_live_overlay, stage_label, stage_artifact_tag) =
        match stage.slug {
            crate::STAGE00_SLUG => {
                let output_root = crate::stage_paths::output_dir_for(&bundle.repo_root, distro_id);
                let s00_spec = load_s00_build_input_spec(
                    distro_id,
                    &bundle.contract.identity.os_name,
                    &bundle.contract.identity.os_id,
                    &output_root,
                )
                .with_context(|| format!("loading 00Build stage baseline for '{}'", distro_id))?;
                let prepared = prepare_s00_build_inputs_for_distro(&s00_spec, output_dir)
                    .with_context(|| format!("preparing 00Build inputs for '{}'", distro_id))?;
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    crate::STAGE00_CANONICAL,
                    crate::STAGE00_ARTIFACT_TAG,
                )
            }
            crate::STAGE01_SLUG => {
                let s01_spec =
                    load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                        .with_context(|| format!("loading 01Boot config for '{}'", distro_id))?;
                let prepared = prepare_s01_boot_inputs_for_distro(&s01_spec, output_dir)
                    .with_context(|| format!("preparing 01Boot inputs for '{}'", distro_id))?;
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    crate::STAGE01_CANONICAL,
                    crate::STAGE01_ARTIFACT_TAG,
                )
            }
            crate::STAGE02_SLUG => {
                let s02_spec = load_s02_live_tools_input_spec(
                    &bundle.repo_root,
                    &bundle.variant_dir,
                    distro_id,
                )
                .with_context(|| format!("loading 02LiveTools config for '{}'", distro_id))?;
                let prepared = prepare_s02_live_tools_inputs_for_distro(&s02_spec, output_dir)
                    .with_context(|| format!("preparing 02LiveTools inputs for '{}'", distro_id))?;
                (
                    prepared.rootfs_source_dir,
                    prepared.live_overlay_dir,
                    crate::STAGE02_CANONICAL,
                    crate::STAGE02_ARTIFACT_TAG,
                )
            }
            _ => unreachable!("validated in parse_stage"),
        };

    let rootfs_source = format!("{}\n", prepared_rootfs_source.display());
    let source_path_file =
        output_dir.join(format!(".{}-live-rootfs-source.path", stage_artifact_tag));
    std::fs::write(&source_path_file, &rootfs_source).with_context(|| {
        format!(
            "writing Stage {} rootfs source path file '{}'",
            stage_label,
            source_path_file.display()
        )
    })?;

    println!("{} inputs prepared:", stage_label);
    println!("  rootfs source: {}", prepared_rootfs_source.display());
    println!("  live overlay:  {}", prepared_live_overlay.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

pub(crate) fn preseed_stage01_source_cmd(distro_id: &str, refresh: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;
    let s01_spec = load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
        .with_context(|| format!("loading 01Boot config for '{}'", distro_id))?;

    if let Some(preseed_recipe_script) = s01_spec.rpm_dvd_preseed_recipe_script() {
        let iso_path =
            preseed_stage01_dvd(&bundle.repo_root, distro_id, preseed_recipe_script, refresh)
                .with_context(|| format!("preseeding Stage 01 source for '{}'", distro_id))?;
        let trust_dir = iso_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                bundle
                    .repo_root
                    .join(".artifacts/work")
                    .join(distro_id)
                    .join("downloads")
            });

        println!("Stage 01 source preseed ready for {}:", distro_id);
        println!("  ISO:   {}", iso_path.display());
        println!("  Trust: {}", trust_dir.display());
        return Ok(());
    }

    if s01_spec.uses_alpine_stage01_rootfs_source() {
        let output = preseed_alpine_stage01_assets(&bundle.repo_root, distro_id, refresh)
            .with_context(|| format!("preseeding Stage 01 source for '{}'", distro_id))?;
        let trust_dir = output
            .iso_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| {
                bundle
                    .repo_root
                    .join(".artifacts/work")
                    .join(distro_id)
                    .join("downloads")
            });

        println!("Stage 01 source preseed ready for {}:", distro_id);
        println!("  ISO:        {}", output.iso_path.display());
        println!("  apk-tools:  {}", output.apk_tools_path.display());
        println!("  Trust:      {}", trust_dir.display());
        return Ok(());
    }

    bail!(
        "Stage 01 for '{}' does not use a canonical preseedable source recipe in '{}'",
        distro_id,
        bundle.variant_dir.join("01Boot.toml").display()
    );
}
