use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use distro_builder::stages::s01_boot_inputs::load_s01_boot_input_spec;
use distro_builder::stages::s02_live_tools_inputs::load_s02_live_tools_input_spec;
use distro_contract::load_stage_00_contract_bundle_for_distro_from;

use crate::workflows::prepared_products::{
    compatibility_prepared_output_names, write_prepared_product_outputs, PreparedProductInputs,
};

pub(crate) fn build_stage_erofs_cmd(stage: &str, distro_id: &str) -> Result<()> {
    let stage = crate::workflows::parse_stage(Some(stage))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;
    let stage_output_dir = crate::stage_paths::compatibility_stage_output_dir_for(
        &bundle.repo_root,
        distro_id,
        stage.dir_name,
    );

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

    crate::workflows::build_prepared_product_erofs_cmd(&stage_output_dir)
}

pub(crate) fn prepare_stage_inputs_cmd(
    stage: &str,
    distro_id: &str,
    output_dir: &Path,
) -> Result<()> {
    let stage = crate::workflows::parse_stage(Some(stage))?;
    let product = crate::workflows::parse::product_for_stage(stage);
    let prepared = prepare_compatibility_product_inputs(product, distro_id, output_dir)?;
    let output_names = compatibility_prepared_output_names(stage);
    let source_path_file =
        write_prepared_product_outputs(output_dir, product, distro_id, &prepared, &output_names)?;

    println!(
        "compatibility inputs prepared for {} {}:",
        stage.canonical, distro_id
    );
    println!("  product:       {}", product.canonical);
    println!("  rootfs source: {}", prepared.rootfs_source_dir.display());
    println!("  live overlay:  {}", prepared.live_overlay_dir.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

fn prepare_compatibility_product_inputs(
    product: crate::BuildProduct,
    distro_id: &str,
    output_dir: &Path,
) -> Result<PreparedProductInputs> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => {
            let output_root =
                crate::stage_paths::distro_output_root_for(&bundle.repo_root, distro_id);
            let spec = distro_builder::stages::s01_boot_inputs::load_s00_build_input_spec(
                distro_id,
                &bundle.contract.identity.os_name,
                &bundle.contract.identity.os_id,
                &output_root,
            )
            .with_context(|| {
                format!("loading {} baseline for '{}'", product.canonical, distro_id)
            })?;
            let prepared = distro_builder::stages::s01_boot_inputs::prepare_s00_build_inputs(
                &spec, output_dir,
            )
            .with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        crate::PRODUCT_LIVE_BOOT => {
            let spec = load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                .with_context(|| {
                    format!("loading {} config for '{}'", product.canonical, distro_id)
                })?;
            let prepared =
                distro_builder::stages::s01_boot_inputs::prepare_s01_boot_inputs(&spec, output_dir)
                    .with_context(|| {
                        format!("preparing {} inputs for '{}'", product.canonical, distro_id)
                    })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        crate::PRODUCT_LIVE_TOOLS => {
            let spec =
                load_s02_live_tools_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                    .with_context(|| {
                    format!("loading {} config for '{}'", product.canonical, distro_id)
                })?;
            let prepared =
                distro_builder::stages::s02_live_tools_inputs::prepare_s02_live_tools_inputs(
                    &spec, output_dir,
                )
                .with_context(|| {
                    format!("preparing {} inputs for '{}'", product.canonical, distro_id)
                })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        _ => unreachable!("validated in parse_product"),
    }
}
