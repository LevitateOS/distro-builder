use std::path::Path;

use anyhow::{Context, Result};
use distro_builder::product::ProductId;
use distro_contract::profile::Flavour;
use distro_builder::recipe::rootfs_source::{preseed_apps_build, preseed_rootfs_source};
use distro_builder::{build_erofs_default, build_overlayfs_default, PreparedProduct};

pub(crate) fn build_prepared_product_erofs_cmd(prepared_dir: &Path) -> Result<()> {
    let manifest = PreparedProduct::read(prepared_dir)?;
    let rootfs_source_dir = manifest.rootfs_source(prepared_dir);
    let overlay_dir = manifest.overlay(prepared_dir);
    let rootfs_output = prepared_dir.join(&manifest.rootfs_erofs);
    let overlay_output = prepared_dir.join(&manifest.overlay_erofs);

    levitate_recipe::target::finalize(&rootfs_source_dir)
        .context("finalizing the prepared system's intended metadata")?;

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
    flavour: Flavour,
    refresh: bool,
) -> Result<()> {
    let bundle = distro_contract::load(repo_root, distro_id)?;
    let recipe_script = distro_builder::preseed_recipe(&bundle, flavour);
    let paths = preseed_rootfs_source(&bundle.repo_root, distro_id, &recipe_script, flavour, refresh)
        .with_context(|| format!("preseeding the {flavour} rootfs source for '{distro_id}'"))?;
    println!("{flavour} rootfs source preseed ready for {distro_id}:");
    for (name, path) in paths {
        println!("  {name}: {}", path.display());
    }
    // The desktop system carries the desktop apps, which are compiled in a
    // root of the variant's own packages: its downloads go with the desktop's.
    if flavour == Flavour::Desktop {
        let recipe_script = distro_builder::apps_build_preseed_recipe(&bundle);
        let paths = preseed_apps_build(&bundle.repo_root, distro_id, &recipe_script, refresh)
            .with_context(|| format!("preseeding what compiling the desktop apps needs for '{distro_id}'"))?;
        println!("apps-build preseed ready for {distro_id}:");
        for (name, path) in paths {
            println!("  {name}: {}", path.display());
        }
    }
    Ok(())
}
