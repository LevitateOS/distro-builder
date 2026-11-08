use std::path::Path;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use distro_builder::recipe::rootfs_source::preseed_rootfs_source;
use distro_builder::{build_erofs_default, build_overlayfs_default};
use distro_builder::product::ProductId;
use distro_builder::{
    load_base_rootfs_product_spec, load_installed_boot_product_spec, load_live_boot_product_spec,
    load_live_tools_product_spec, materialize_live_boot_source_rootfs, prepare_base_rootfs_product,
    prepare_installed_boot_product, prepare_live_boot_product, prepare_live_tools_product,
    BaseProductLayout, DerivedProductLayout, OverlayLayout,
};
use distro_contract::Variant;

use crate::workflows::prepared_products::{
    prepared_output_names, read_prepared_product_manifest, resolve_prepared_product_path,
    write_prepared_product_outputs, PreparedProductInputs,
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

fn base_product_layout(product: ProductId) -> BaseProductLayout {
    BaseProductLayout {
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        live_overlay_dir_name: distro_builder::product::build(product).overlay_dir.to_string(),
    }
}

/// A product built on its parent's rootfs.
fn derived_product_layout(product: ProductId) -> DerivedProductLayout {
    DerivedProductLayout {
        product,
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        live_overlay: OverlayLayout {
            issue_banner_label: product.title().to_string(),
            dir_name: distro_builder::product::build(product).overlay_dir.to_string(),
        },
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
    let product = crate::workflows::parse_product(product)?;
    let repo_root = crate::workflows::repo_root()?;
    let bundle = distro_contract::load(&repo_root, distro_id)?;
    crate::workflows::ensure_release_prerequisites(&bundle.repo_root, distro_id, product)
        .with_context(|| {
            format!("realizing parent release prerequisites for product '{product}' on '{distro_id}'")
        })?;
    let parent_rootfs = distro_builder::parent_rootfs_image(
        &bundle.repo_root,
        distro_id,
        &bundle.contract,
        product,
    )
    .with_context(|| format!("resolving the parent rootfs of '{product}' on '{distro_id}'"))?;
    let prepared = prepare_product_inputs(&bundle, product, distro_id, output_dir, parent_rootfs)?;
    let output_names = prepared_output_names(&bundle.contract, product);
    let source_path_file =
        write_prepared_product_outputs(output_dir, product, distro_id, &prepared, &output_names)?;

    println!("product '{product}' inputs prepared for {distro_id}:");
    println!("  rootfs source: {}", prepared.rootfs_source_dir.display());
    println!("  live overlay:  {}", prepared.live_overlay_dir.display());
    println!("  source path:   {}", source_path_file.display());
    Ok(())
}

/// `parent_rootfs` is the parent product's built rootfs image (`None` for
/// base-rootfs, which has no parent).
fn prepare_product_inputs(
    bundle: &Variant,
    product: ProductId,
    distro_id: &str,
    output_dir: &Path,
    parent_rootfs: Option<PathBuf>,
) -> Result<PreparedProductInputs> {
    let loading = || format!("loading {product} config for '{distro_id}'");
    let preparing = || format!("preparing {product} inputs for '{distro_id}'");
    let with_parent = |spec_parent: Option<PathBuf>| {
        spec_parent.ok_or_else(|| anyhow::anyhow!("{product} on '{distro_id}' has no parent rootfs"))
    };
    let (rootfs_source_dir, live_overlay_dir) = match product {
        ProductId::BaseRootfs => {
            let spec = load_base_rootfs_product_spec(
                distro_id,
                &bundle.contract.identity.os_name,
                &bundle.contract.identity.os_id,
                &bundle.contract.profile,
                base_product_layout(product),
            );
            let prepared = prepare_base_rootfs_product(&spec, output_dir).with_context(preparing)?;
            (prepared.rootfs_source_dir, prepared.live_overlay_dir)
        }
        ProductId::LiveBoot => {
            let spec = load_live_boot_product_spec(
                &bundle.repo_root,
                &bundle.contract,
                distro_id,
                derived_product_layout(product),
            )
            .with_context(loading)?
            .with_resolved_parent_rootfs_image(with_parent(parent_rootfs)?);
            let prepared = prepare_live_boot_product(&spec, output_dir).with_context(preparing)?;
            (prepared.rootfs_source_dir, prepared.live_overlay_dir)
        }
        ProductId::LiveTools => {
            let spec = load_live_tools_product_spec(
                &bundle.repo_root,
                &bundle.contract,
                distro_id,
                derived_product_layout(product),
            )
            .with_context(loading)?
            .with_resolved_parent_rootfs_image(with_parent(parent_rootfs)?);
            let prepared = prepare_live_tools_product(&spec, output_dir).with_context(preparing)?;
            (prepared.rootfs_source_dir, prepared.live_overlay_dir)
        }
        ProductId::InstalledBoot => {
            let spec = load_installed_boot_product_spec(
                &bundle.repo_root,
                &bundle.contract,
                distro_id,
                derived_product_layout(product),
            )
            .with_context(loading)?
            .with_resolved_parent_rootfs_image(with_parent(parent_rootfs)?);
            let prepared =
                prepare_installed_boot_product(&spec, output_dir).with_context(preparing)?;
            (prepared.rootfs_source_dir, prepared.live_overlay_dir)
        }
    };
    Ok(PreparedProductInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub(crate) fn preseed_rootfs_source_cmd(distro_id: &str, refresh: bool) -> Result<()> {
    let repo_root = crate::workflows::repo_root()?;
    let bundle = distro_contract::load(&repo_root, distro_id)?;
    let live_boot_spec = canonical_live_boot_product_spec(&bundle, distro_id)
        .with_context(|| format!("loading canonical rootfs source policy for '{}'", distro_id))?;

    let (recipe_script, outputs) = live_boot_spec.rootfs_source_preseed().with_context(|| {
        format!("the {distro_id} rootfs source has no preseed recipe")
    })?;
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
    let repo_root = crate::workflows::repo_root()?;
    let bundle = distro_contract::load(&repo_root, distro_id)?;
    let live_boot_spec = canonical_live_boot_product_spec(&bundle, distro_id)
        .with_context(|| format!("loading canonical rootfs source policy for '{}'", distro_id))?;

    let source_rootfs_dir = materialize_live_boot_source_rootfs(&live_boot_spec)
        .with_context(|| format!("materializing canonical rootfs source for '{}'", distro_id))?;

    println!("rootfs source ready for {}:", distro_id);
    println!("  rootfs source: {}", source_rootfs_dir.display());
    Ok(())
}

fn canonical_live_boot_product_spec(
    bundle: &Variant,
    distro_id: &str,
) -> Result<distro_builder::LiveBootProductSpec> {
    load_live_boot_product_spec(
        &bundle.repo_root,
        &bundle.contract,
        distro_id,
        derived_product_layout(ProductId::LiveBoot),
    )
}
