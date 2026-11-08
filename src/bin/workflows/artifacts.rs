use std::path::Path;

use anyhow::{Context, Result};
use distro_builder::product::ProductId;
use distro_builder::recipe::rootfs_source::preseed_rootfs_source;
use distro_builder::{build_erofs_default, build_overlayfs_default, PreparedProduct};

pub(crate) fn build_prepared_product_erofs_cmd(prepared_dir: &Path) -> Result<()> {
    let manifest = PreparedProduct::read(prepared_dir)?;
    let rootfs_source_dir = manifest.rootfs_source(prepared_dir);
    let overlay_dir = manifest.overlay(prepared_dir);
    let rootfs_output = prepared_dir.join(&manifest.rootfs_erofs);
    let overlay_output = prepared_dir.join(&manifest.overlay_erofs);

    build_erofs_default(&rootfs_source_dir, &rootfs_output).with_context(|| {
        format!("building the {} rootfs EROFS", manifest.product)
    })?;
    build_overlayfs_default(&overlay_dir, &overlay_output).with_context(|| {
        format!("building the {} overlay EROFS", manifest.product)
    })?;

    println!(
        "product '{}' EROFS artifacts built for {}:",
        manifest.product, manifest.distro_id
    );
    println!("  prepared dir: {}", prepared_dir.display());
    println!("  rootfs source: {}", rootfs_source_dir.display());
    println!("  overlay source: {}", overlay_dir.display());
    println!("  rootfs erofs: {}", rootfs_output.display());
    println!("  overlay erofs: {}", overlay_output.display());
    Ok(())
}

pub(crate) fn prepare_product_cmd(
    repo_root: &Path,
    product: ProductId,
    distro_id: &str,
    output_dir: &Path,
) -> Result<()> {
    let bundle = distro_contract::load(repo_root, distro_id)?;
    super::build::ensure_release_prerequisites(&bundle.repo_root, distro_id, product)
        .with_context(|| {
            format!("realizing parent release prerequisites for product '{product}' on '{distro_id}'")
        })?;
    let prepared = distro_builder::prepare_product(&bundle, product, output_dir)
        .with_context(|| format!("preparing {product} for '{distro_id}'"))?;

    println!("product '{product}' inputs prepared for {distro_id}:");
    println!("  rootfs source: {}", prepared.rootfs_source(output_dir).display());
    println!("  overlay:       {}", prepared.overlay(output_dir).display());
    Ok(())
}

pub(crate) fn preseed_rootfs_source_cmd(
    repo_root: &Path,
    distro_id: &str,
    refresh: bool,
) -> Result<()> {
    let bundle = distro_contract::load(repo_root, distro_id)?;
    let source = &bundle.contract.sources.rootfs_source;
    let recipe_script = source
        .preseed_recipe_script
        .as_deref()
        .with_context(|| format!("the {distro_id} rootfs source has no preseed recipe"))?;
    let paths = preseed_rootfs_source(
        &bundle.repo_root,
        distro_id,
        Path::new(recipe_script),
        refresh,
    )
    .with_context(|| format!("preseeding rootfs source for '{distro_id}'"))?;
    println!("rootfs source preseed ready for {distro_id}:");
    for (name, path) in paths {
        println!("  {name}: {}", path.display());
    }
    Ok(())
}
