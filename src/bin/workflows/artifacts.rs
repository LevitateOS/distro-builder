use std::path::Path;
use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use distro_builder::recipe::alpine_stage01::preseed_alpine_stage01_assets;
use distro_builder::recipe::stage01_source::preseed_stage01_dvd;
use distro_builder::stages::s01_boot_inputs::{
    load_s00_build_input_spec, load_s01_boot_input_spec, materialize_s01_source_rootfs,
    prepare_s00_build_inputs as prepare_s00_build_inputs_for_distro,
    prepare_s01_boot_inputs as prepare_s01_boot_inputs_for_distro,
};
use distro_builder::stages::s02_live_tools_inputs::{
    load_s02_live_tools_input_spec,
    prepare_s02_live_tools_inputs as prepare_s02_live_tools_inputs_for_distro,
};
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_contract::load_stage_00_contract_bundle_for_distro_from;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct PreparedProductInputs {
    rootfs_source_dir: PathBuf,
    live_overlay_dir: PathBuf,
    compatibility_stage: crate::BuildStage,
}

#[derive(Debug, Serialize, Deserialize)]
struct PreparedProductManifest {
    product: String,
    distro_id: String,
    compatibility_stage_name: String,
    compatibility_stage_slug: String,
    rootfs_source_dir: String,
    live_overlay_dir: String,
    rootfs_erofs_filename: String,
    overlay_erofs_filename: String,
}

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

    build_prepared_product_erofs_cmd(&stage_output_dir)
}

pub(crate) fn build_prepared_product_erofs_cmd(prepared_dir: &Path) -> Result<()> {
    let manifest = read_prepared_product_manifest(prepared_dir)?;
    let rootfs_source_dir =
        resolve_prepared_product_path(prepared_dir, &manifest.rootfs_source_dir);
    let live_overlay_dir = resolve_prepared_product_path(prepared_dir, &manifest.live_overlay_dir);
    if !live_overlay_dir.is_dir() {
        bail!(
            "live overlay source directory missing for product '{}' at '{}'\n\
             Remediation: rerun `distro-builder product prepare {} {} {}` and verify overlay preparation succeeds.",
            manifest.product,
            live_overlay_dir.display(),
            manifest.product,
            manifest.distro_id,
            prepared_dir.display()
        );
    }
    if !rootfs_source_dir.is_dir() {
        bail!(
            "rootfs source directory missing for product '{}' at '{}'\n\
             Remediation: rerun `distro-builder product prepare {} {} {}` and verify rootfs preparation succeeds.",
            manifest.product,
            rootfs_source_dir.display(),
            manifest.product,
            manifest.distro_id,
            prepared_dir.display()
        );
    }

    let rootfs_output = prepared_dir.join(&manifest.rootfs_erofs_filename);
    let overlay_output = prepared_dir.join(&manifest.overlay_erofs_filename);

    build_rootfs_erofs(&rootfs_source_dir, &rootfs_output).with_context(|| {
        format!(
            "building product '{}' rootfs EROFS from '{}' to '{}'",
            manifest.product,
            rootfs_source_dir.display(),
            rootfs_output.display()
        )
    })?;
    build_overlayfs_erofs(&live_overlay_dir, &overlay_output).with_context(|| {
        format!(
            "building product '{}' overlayfs EROFS from '{}' to '{}'",
            manifest.product,
            live_overlay_dir.display(),
            overlay_output.display()
        )
    })?;

    println!(
        "product '{}' EROFS artifacts built for {}:",
        manifest.product, manifest.distro_id
    );
    println!(
        "  compatibility stage: {}",
        manifest.compatibility_stage_name
    );
    println!("  prepared dir: {}", prepared_dir.display());
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
    let product = crate::workflows::parse::product_for_stage(stage);
    prepare_product_cmd(product.canonical, distro_id, output_dir)
}

pub(crate) fn prepare_product_cmd(product: &str, distro_id: &str, output_dir: &Path) -> Result<()> {
    let product = crate::workflows::parse_product(Some(product))?;
    let prepared = prepare_product_inputs(product, distro_id, output_dir)?;
    let stage_artifact_tag = stage_artifact_tag_for_slug(prepared.compatibility_stage.slug);
    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file =
        output_dir.join(format!(".{}-live-rootfs-source.path", stage_artifact_tag));
    std::fs::write(&source_path_file, &rootfs_source).with_context(|| {
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
            compatibility_stage_name: prepared.compatibility_stage.canonical.to_string(),
            compatibility_stage_slug: prepared.compatibility_stage.slug.to_string(),
            rootfs_source_dir: relative_prepared_product_path(
                output_dir,
                &prepared.rootfs_source_dir,
            )?,
            live_overlay_dir: relative_prepared_product_path(
                output_dir,
                &prepared.live_overlay_dir,
            )?,
            rootfs_erofs_filename: format!("{stage_artifact_tag}-filesystem.erofs"),
            overlay_erofs_filename: format!("{stage_artifact_tag}-overlayfs.erofs"),
        },
    )?;

    println!(
        "product '{}' inputs prepared for {}:",
        product.canonical, distro_id
    );
    println!(
        "  compatibility stage: {}",
        prepared.compatibility_stage.canonical
    );
    println!("  rootfs source: {}", prepared.rootfs_source_dir.display());
    println!("  live overlay:  {}", prepared.live_overlay_dir.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

fn prepare_product_inputs(
    product: crate::BuildProduct,
    distro_id: &str,
    output_dir: &Path,
) -> Result<PreparedProductInputs> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    let stage = product.compatibility_stage;
    match stage.slug {
        crate::STAGE00_SLUG => {
            let output_root = crate::stage_paths::output_dir_for(&bundle.repo_root, distro_id);
            let s00_spec = load_s00_build_input_spec(
                distro_id,
                &bundle.contract.identity.os_name,
                &bundle.contract.identity.os_id,
                &output_root,
            )
            .with_context(|| {
                format!("loading {} baseline for '{}'", product.canonical, distro_id)
            })?;
            let prepared = prepare_s00_build_inputs_for_distro(&s00_spec, output_dir)
                .with_context(|| {
                    format!("preparing {} inputs for '{}'", product.canonical, distro_id)
                })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
                compatibility_stage: stage,
            })
        }
        crate::STAGE01_SLUG => {
            let s01_spec =
                load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                    .with_context(|| {
                        format!("loading {} config for '{}'", product.canonical, distro_id)
                    })?;
            let prepared =
                prepare_s01_boot_inputs_for_distro(&s01_spec, output_dir).with_context(|| {
                    format!("preparing {} inputs for '{}'", product.canonical, distro_id)
                })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
                compatibility_stage: stage,
            })
        }
        crate::STAGE02_SLUG => {
            let s02_spec =
                load_s02_live_tools_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
                    .with_context(|| {
                    format!("loading {} config for '{}'", product.canonical, distro_id)
                })?;
            let prepared = prepare_s02_live_tools_inputs_for_distro(&s02_spec, output_dir)
                .with_context(|| {
                    format!("preparing {} inputs for '{}'", product.canonical, distro_id)
                })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
                compatibility_stage: stage,
            })
        }
        _ => unreachable!("validated in parse_stage"),
    }
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

fn read_prepared_product_manifest(prepared_dir: &Path) -> Result<PreparedProductManifest> {
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

fn resolve_prepared_product_path(prepared_dir: &Path, relative: &str) -> PathBuf {
    prepared_dir.join(relative)
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

pub(crate) fn materialize_stage01_source_rootfs_cmd(distro_id: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;
    let s01_spec = load_s01_boot_input_spec(&bundle.repo_root, &bundle.variant_dir, distro_id)
        .with_context(|| format!("loading 01Boot config for '{}'", distro_id))?;

    let source_rootfs_dir = materialize_s01_source_rootfs(&s01_spec).with_context(|| {
        format!(
            "materializing canonical Stage 01 source rootfs for '{}'",
            distro_id
        )
    })?;

    println!("Stage 01 source rootfs ready for {}:", distro_id);
    println!("  rootfs source: {}", source_rootfs_dir.display());
    Ok(())
}
