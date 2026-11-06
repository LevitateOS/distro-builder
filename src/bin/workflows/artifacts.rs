use std::path::Path;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use distro_builder::recipe::rootfs_source::preseed_rootfs_source;
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_builder::{
    load_base_rootfs_product_spec, load_installed_boot_product_spec, load_live_boot_product_spec,
    load_live_tools_product_spec, materialize_live_boot_source_rootfs, plan_product_realization,
    prepare_base_rootfs_product, prepare_installed_boot_product, prepare_live_boot_product,
    prepare_live_tools_product, BaseProductLayout, DerivedProductLayout, OverlayLayout,
    ParentRootfsInput, ProductRealizationStep,
};
use distro_contract::{
    load_variant_contract_bundle_for_distro_from, require_valid_contract, ConformanceContract,
    LoadedVariantContract, ProductDecl,
};

use crate::workflows::prepared_products::{
    canonical_prepared_output_names, canonical_rootfs_erofs_filename,
    read_prepared_product_manifest, resolve_prepared_product_path, write_prepared_product_outputs,
    PreparedProductInputs,
};

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
    contract: &ConformanceContract,
    product: crate::BuildProduct,
) -> Result<DerivedProductLayout> {
    let runtime_product = runtime_product_decl(contract, product)?;
    let parent_logical_name = runtime_product
        .extends
        .as_deref()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical Ring 2 composition edge for '{}': product '{}' must declare `extends` in ring2/products.toml",
                product.canonical,
                runtime_product.logical_name
            )
        })?;
    let parent_product = crate::workflows::product_for_logical_name(parent_logical_name)?;
    Ok(DerivedProductLayout {
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        parent_rootfs: ParentRootfsInput {
            release_dir_name: parent_product.release_dir_name.to_string(),
            producer_label: parent_product.canonical.to_string(),
            rootfs_filename: canonical_rootfs_erofs_filename(contract)?,
        },
        live_overlay: OverlayLayout {
            issue_banner_label: product.issue_banner_label.to_string(),
            dir_name: product.live_overlay_dir_name.to_string(),
        },
    })
}

fn runtime_product_decl(
    contract: &ConformanceContract,
    product: crate::BuildProduct,
) -> Result<&ProductDecl> {
    match product.canonical {
        crate::PRODUCT_LIVE_BOOT => Ok(&contract.products.boot_live),
        crate::PRODUCT_LIVE_TOOLS => Ok(&contract.products.live_tools),
        crate::PRODUCT_INSTALLED_BOOT => contract.products.boot_installed.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical Ring 2 product declaration for '{}': ring2/products.toml must define `boot_installed`",
                product.canonical
            )
        }),
        _ => unreachable!("canonical derived product layout is only valid for derived products"),
    }
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

pub(crate) fn prepare_product_cmd(product: &str, distro_id: &str, output_dir: &Path) -> Result<()> {
    let product = crate::workflows::parse_product(Some(product))?;
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_variant_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading canonical variant contract for '{}'", distro_id))?;
    crate::workflows::ensure_release_prerequisites(&bundle.repo_root, distro_id, product)
        .with_context(|| {
            format!(
                "realizing parent release prerequisites for product '{}' on '{}'",
                product.canonical, distro_id
            )
        })?;
    let realization_plan = plan_product_realization(
        &bundle.repo_root,
        distro_id,
        &bundle.contract,
        product.canonical,
    )
    .with_context(|| {
        format!(
            "planning canonical product realization for '{}' on '{}'",
            product.canonical, distro_id
        )
    })?;
    let planned_step = realization_plan.requested_step().with_context(|| {
        format!(
            "looking up requested realization step for canonical product '{}' on '{}'",
            product.canonical, distro_id
        )
    })?;
    let prepared = prepare_product_inputs(&bundle, product, distro_id, output_dir, planned_step)?;
    let output_names = canonical_prepared_output_names(&bundle.contract, product)
        .with_context(|| format!("resolving canonical Ring 1 outputs for '{}'", distro_id))?;
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
    bundle: &LoadedVariantContract,
    product: crate::BuildProduct,
    distro_id: &str,
    output_dir: &Path,
    planned_step: &ProductRealizationStep,
) -> Result<PreparedProductInputs> {
    if planned_step.product != product.canonical {
        bail!(
            "planner mismatch while preparing canonical product '{}' on '{}': requested step resolved '{}' instead\nRemediation: regenerate the planner-backed realization chain before calling the preparer",
            product.canonical,
            distro_id,
            planned_step.product
        );
    }

    match product.canonical {
        crate::PRODUCT_BASE_ROOTFS => {
            let output_root =
                crate::artifact_paths::distro_output_root_for(&bundle.repo_root, distro_id);
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
        crate::PRODUCT_LIVE_BOOT => {
            let layout = canonical_derived_product_layout(&bundle.contract, product)?;
            let resolved_parent_rootfs_image =
                planned_parent_rootfs_image(planned_step, product, distro_id, &layout)?;
            let spec = load_live_boot_product_spec(
                &bundle.repo_root,
                &bundle.variant_dir,
                &bundle.contract,
                distro_id,
                layout,
            )
            .map(|spec| spec.with_resolved_parent_rootfs_image(resolved_parent_rootfs_image))
            .with_context(|| format!("loading {} config for '{}'", product.canonical, distro_id))?;
            let prepared = prepare_live_boot_product(&spec, output_dir).with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        crate::PRODUCT_LIVE_TOOLS => {
            let layout = canonical_derived_product_layout(&bundle.contract, product)?;
            let resolved_parent_rootfs_image =
                planned_parent_rootfs_image(planned_step, product, distro_id, &layout)?;
            let spec = load_live_tools_product_spec(
                &bundle.repo_root,
                &bundle.variant_dir,
                &bundle.contract,
                distro_id,
                layout,
            )
            .map(|spec| spec.with_resolved_parent_rootfs_image(resolved_parent_rootfs_image))
            .with_context(|| format!("loading {} config for '{}'", product.canonical, distro_id))?;
            let prepared = prepare_live_tools_product(&spec, output_dir).with_context(|| {
                format!("preparing {} inputs for '{}'", product.canonical, distro_id)
            })?;
            Ok(PreparedProductInputs {
                rootfs_source_dir: prepared.rootfs_source_dir,
                live_overlay_dir: prepared.live_overlay_dir,
            })
        }
        crate::PRODUCT_INSTALLED_BOOT => {
            let layout = canonical_derived_product_layout(&bundle.contract, product)?;
            let resolved_parent_rootfs_image =
                planned_parent_rootfs_image(planned_step, product, distro_id, &layout)?;
            let spec = load_installed_boot_product_spec(
                &bundle.repo_root,
                &bundle.variant_dir,
                &bundle.contract,
                distro_id,
                layout,
            )
            .map(|spec| spec.with_resolved_parent_rootfs_image(resolved_parent_rootfs_image))
            .with_context(|| format!("loading {} config for '{}'", product.canonical, distro_id))?;
            let prepared =
                prepare_installed_boot_product(&spec, output_dir).with_context(|| {
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

fn planned_parent_rootfs_image(
    planned_step: &ProductRealizationStep,
    product: crate::BuildProduct,
    distro_id: &str,
    layout: &DerivedProductLayout,
) -> Result<PathBuf> {
    planned_step
        .resolved_parent_rootfs_image
        .clone()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "planner did not resolve parent release '{}' for canonical product '{}' on '{}'\nRemediation: use the planner-backed `distro-builder product prepare {} {} <output_dir>` entrypoint after parent release realization succeeds",
                layout.parent_rootfs.producer_label,
                product.canonical,
                distro_id,
                product.canonical,
                distro_id
            )
        })
}

pub(crate) fn preseed_rootfs_source_cmd(distro_id: &str, refresh: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_variant_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading canonical variant contract for '{}'", distro_id))?;
    let live_boot_spec = canonical_live_boot_product_spec(&bundle, distro_id)
        .with_context(|| format!("loading canonical rootfs source policy for '{}'", distro_id))?;

    let (recipe_script, outputs) = live_boot_spec.rootfs_source_preseed().with_context(|| format!(
        "rootfs source for '{distro_id}' does not declare sources.rootfs_source.preseed_recipe_script in '{}'",
        bundle.paths.ring3_sources_manifest.display()))?;
    let paths = preseed_rootfs_source(
        &bundle.repo_root,
        distro_id,
        recipe_script,
        outputs,
        &bundle.contract.sources.rootfs_source.preseed_network_hosts,
        bundle.contract.sources.rootfs_source.preseed_memory_mib,
        refresh,
    )
    .with_context(|| format!("preseeding rootfs source for '{distro_id}'"))?;
    println!("rootfs source preseed ready for {distro_id}:");
    for (name, path) in paths {
        println!("  {name}: {}", path.display());
    }
    Ok(())
}

pub(crate) fn materialize_rootfs_source_cmd(distro_id: &str) -> Result<()> {
    let cwd = std::env::current_dir().context("resolving current directory")?;
    let bundle = load_variant_contract_bundle_for_distro_from(&cwd, distro_id)
        .with_context(|| format!("loading canonical variant contract for '{}'", distro_id))?;
    let live_boot_spec = canonical_live_boot_product_spec(&bundle, distro_id)
        .with_context(|| format!("loading canonical rootfs source policy for '{}'", distro_id))?;

    let source_rootfs_dir = materialize_live_boot_source_rootfs(&live_boot_spec)
        .with_context(|| format!("materializing canonical rootfs source for '{}'", distro_id))?;

    println!("rootfs source ready for {}:", distro_id);
    println!("  rootfs source: {}", source_rootfs_dir.display());
    Ok(())
}

fn canonical_live_boot_product_spec(
    bundle: &LoadedVariantContract,
    distro_id: &str,
) -> Result<distro_builder::LiveBootProductSpec> {
    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating canonical variant contract for '{distro_id}'"))?;
    let product = crate::workflows::parse_product(Some(crate::PRODUCT_LIVE_BOOT))?;
    let layout = canonical_derived_product_layout(&bundle.contract, product)?;
    load_live_boot_product_spec(
        &bundle.repo_root,
        &bundle.variant_dir,
        &bundle.contract,
        distro_id,
        layout,
    )
}
