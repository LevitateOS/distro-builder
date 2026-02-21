use std::path::Path;

use anyhow::{Context, Result};
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
