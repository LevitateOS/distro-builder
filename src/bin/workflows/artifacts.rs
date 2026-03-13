use std::path::Path;
use std::{fs, path::PathBuf};

use anyhow::{bail, Context, Result};
use distro_builder::recipe::alpine_stage01::preseed_alpine_stage01_assets;
use distro_builder::recipe::stage01_source::preseed_stage01_dvd;
use distro_builder::stages::s01_boot_inputs::{
    load_s01_boot_input_spec, materialize_s01_source_rootfs,
};
use distro_builder::stages::s02_live_tools_inputs::load_s02_live_tools_input_spec;
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_builder::{
    load_base_rootfs_product_spec, load_live_boot_product_spec, load_live_tools_product_spec,
    prepare_base_rootfs_product, prepare_live_boot_product, prepare_live_tools_product,
    BaseProductLayout, DerivedProductLayout, OverlayLayout, ParentRootfsInput,
};
use distro_contract::load_stage_00_contract_bundle_for_distro_from;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
struct PreparedProductInputs {
    rootfs_source_dir: PathBuf,
    live_overlay_dir: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct PreparedProductManifest {
    product: String,
    distro_id: String,
    rootfs_source_dir: String,
    live_overlay_dir: String,
    rootfs_source_pointer_filename: String,
    rootfs_erofs_filename: String,
    overlay_erofs_filename: String,
}

#[derive(Debug, Clone, Copy)]
enum PreparationMode {
    Canonical,
    CompatibilityStage(crate::BuildStage),
}

#[derive(Debug, Clone)]
struct PreparedOutputNames {
    rootfs_source_pointer_filename: String,
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

fn canonical_base_product_layout(product: crate::BuildProduct) -> BaseProductLayout {
    BaseProductLayout {
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        live_overlay_dir_name: product.live_overlay_dir_name.to_string(),
    }
}

fn canonical_derived_product_layout(
    product: crate::BuildProduct,
    parent_product: crate::BuildProduct,
) -> DerivedProductLayout {
    DerivedProductLayout {
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        parent_rootfs: ParentRootfsInput {
            release_dir_name: parent_product.release_dir_name.to_string(),
            producer_label: parent_product.canonical.to_string(),
            rootfs_filename: parent_product.rootfs_erofs_filename.to_string(),
        },
        live_overlay: OverlayLayout {
            issue_banner_label: product.issue_banner_label.to_string(),
            dir_name: product.live_overlay_dir_name.to_string(),
        },
    }
}

fn prepared_output_names(
    product: crate::BuildProduct,
    mode: PreparationMode,
) -> PreparedOutputNames {
    match mode {
        PreparationMode::Canonical => PreparedOutputNames {
            rootfs_source_pointer_filename: product.rootfs_source_pointer_filename.to_string(),
            rootfs_erofs_filename: product.rootfs_erofs_filename.to_string(),
            overlay_erofs_filename: product.overlay_erofs_filename.to_string(),
        },
        PreparationMode::CompatibilityStage(stage) => PreparedOutputNames {
            rootfs_source_pointer_filename: format!(
                ".{}-live-rootfs-source.path",
                stage.artifact_tag
            ),
            rootfs_erofs_filename: format!("{}-filesystem.erofs", stage.artifact_tag),
            overlay_erofs_filename: format!("{}-overlayfs.erofs", stage.artifact_tag),
        },
    }
}

fn write_prepared_product_outputs(
    output_dir: &Path,
    product: crate::BuildProduct,
    distro_id: &str,
    prepared: &PreparedProductInputs,
    output_names: &PreparedOutputNames,
) -> Result<PathBuf> {
    let rootfs_source = format!("{}\n", prepared.rootfs_source_dir.display());
    let source_path_file = output_dir.join(&output_names.rootfs_source_pointer_filename);
    fs::write(&source_path_file, &rootfs_source).with_context(|| {
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
    let prepared = prepare_product_inputs(
        product,
        distro_id,
        output_dir,
        PreparationMode::CompatibilityStage(stage),
    )?;
    let output_names = prepared_output_names(product, PreparationMode::CompatibilityStage(stage));
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

pub(crate) fn prepare_product_cmd(product: &str, distro_id: &str, output_dir: &Path) -> Result<()> {
    let product = crate::workflows::parse_product(Some(product))?;
    let prepared =
        prepare_product_inputs(product, distro_id, output_dir, PreparationMode::Canonical)?;
    let output_names = prepared_output_names(product, PreparationMode::Canonical);
    let source_path_file =
        write_prepared_product_outputs(output_dir, product, distro_id, &prepared, &output_names)?;

    println!(
        "product '{}' inputs prepared for {}:",
        product.canonical, distro_id
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
    mode: PreparationMode,
) -> Result<PreparedProductInputs> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_stage_00_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading 00Build contract for '{}'", distro_id))?;

    match (product.canonical, mode) {
        (crate::PRODUCT_BASE_ROOTFS, PreparationMode::Canonical) => {
            let output_root = crate::stage_paths::output_dir_for(&bundle.repo_root, distro_id);
            let spec = load_base_rootfs_product_spec(
                distro_id,
                &bundle.contract.identity.os_name,
                &bundle.contract.identity.os_id,
                &output_root,
                canonical_base_product_layout(product),
            )
            .with_context(|| {
                format!("loading {} baseline for '{}'", product.canonical, distro_id)
            })?;
            let prepared = prepare_base_rootfs_product(&spec, output_dir).with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        (crate::PRODUCT_BASE_ROOTFS, PreparationMode::CompatibilityStage(_)) => {
            let output_root = crate::stage_paths::output_dir_for(&bundle.repo_root, distro_id);
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
        (crate::PRODUCT_LIVE_BOOT, PreparationMode::Canonical) => {
            let layout = canonical_derived_product_layout(
                product,
                crate::workflows::parse_product(Some(crate::PRODUCT_BASE_ROOTFS))?,
            );
            let spec = load_live_boot_product_spec(
                &bundle.repo_root,
                &bundle.variant_dir,
                distro_id,
                layout,
            )
            .with_context(|| format!("loading {} config for '{}'", product.canonical, distro_id))?;
            let prepared = prepare_live_boot_product(&spec, output_dir).with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        (crate::PRODUCT_LIVE_BOOT, PreparationMode::CompatibilityStage(_)) => {
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
        (crate::PRODUCT_LIVE_TOOLS, PreparationMode::Canonical) => {
            let live_boot_product =
                crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_BOOT))?;
            let layout = canonical_derived_product_layout(product, live_boot_product);
            let spec = load_live_tools_product_spec(
                &bundle.repo_root,
                &bundle.variant_dir,
                distro_id,
                layout,
            )
            .with_context(|| format!("loading {} config for '{}'", product.canonical, distro_id))?;
            let prepared = prepare_live_tools_product(&spec, output_dir).with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        (crate::PRODUCT_LIVE_TOOLS, PreparationMode::CompatibilityStage(_)) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_prepared_output_names_are_product_native() {
        let product = crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_BOOT))
            .expect("parse live-boot");
        let names = prepared_output_names(product, PreparationMode::Canonical);
        assert_eq!(
            names.rootfs_source_pointer_filename,
            ".live-rootfs-source.path"
        );
        assert_eq!(names.rootfs_erofs_filename, "filesystem.erofs");
        assert_eq!(names.overlay_erofs_filename, "overlayfs.erofs");
    }

    #[test]
    fn compatibility_prepared_output_names_preserve_stage_artifacts() {
        let stage = crate::workflows::parse_stage(Some("01Boot")).expect("parse stage");
        let product = crate::workflows::parse::product_for_stage(stage);
        let names = prepared_output_names(product, PreparationMode::CompatibilityStage(stage));
        assert_eq!(
            names.rootfs_source_pointer_filename,
            ".s01-live-rootfs-source.path"
        );
        assert_eq!(names.rootfs_erofs_filename, "s01-filesystem.erofs");
        assert_eq!(names.overlay_erofs_filename, "s01-overlayfs.erofs");
    }
}
