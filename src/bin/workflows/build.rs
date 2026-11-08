use anyhow::{Context, Result};
use std::path::Path;

use distro_builder::product::{ProductId, ReleaseProduct};

use distro_contract::runs;

/// Build the releases `product` is built on that have no rootfs yet.
pub(crate) fn ensure_release_prerequisites(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
) -> Result<()> {
    let variant = distro_contract::load(repo_root, distro_id)?;
    let missing =
        distro_builder::missing_parent_releases(repo_root, distro_id, &variant.contract, product)?;
    for parent in missing {
        let release = ReleaseProduct::new(parent)
            .with_context(|| format!("{product} is built on {parent}, which has no release"))?;
        println!(
            "[release:iso:{product}:{distro_id}] materializing missing parent release '{parent}'..."
        );
        build_one(distro_id, release)?;
    }

    Ok(())
}

pub(crate) fn build_all(release: ReleaseProduct) -> Result<()> {
    let product = release.product;
    let repo_root = crate::workflows::repo_root()?;
    let distro_ids = crate::workflows::parse::discover_distro_ids(&repo_root)?;
    for distro_id in &distro_ids {
        println!("[release:iso:{product}] building {distro_id}...");
        ensure_release_prerequisites(&repo_root, distro_id, product)?;
        build_one(distro_id, release)?;
    }
    Ok(())
}

pub(crate) fn build_one(distro_id: &str, release: ReleaseProduct) -> Result<()> {
    let product = release.product;
    let repo_root = crate::workflows::repo_root()?;
    let bundle = distro_contract::load(&repo_root, distro_id)?;

    let kernel_output_dir = runs::kernel_dir(&bundle.repo_root, distro_id);
    std::fs::create_dir_all(&kernel_output_dir).with_context(|| {
        format!(
            "creating kernel output directory '{}'",
            kernel_output_dir.display()
        )
    })?;
    let mut run = runs::Run::start_release(&bundle.repo_root, distro_id, product)?;
    let iso_path = run.dir.join(bundle.contract.artifacts.iso(product));
    run.manifest.iso_path = Some(iso_path.clone());
    run.save()?;

    let build_result = (|| -> Result<()> {
        distro_builder::kernel::ensure_kernel_preinstalled(&bundle, &kernel_output_dir)
            .with_context(|| format!("ensuring kernel artifacts for '{distro_id}'"))?;
        println!("[release:iso:{product}:{distro_id}] kernel already installed");
        crate::workflows::ensure_release_iso_via_variant_hook(
            &bundle,
            distro_id,
            &kernel_output_dir,
            &run,
            release,
        )?;

        println!(
            "[release:iso:{product}:{distro_id}] built at {}",
            iso_path.display()
        );
        Ok(())
    })();

    let release_dir = runs::release_dir(&bundle.repo_root, distro_id, product);
    let recorded = run
        .finish(build_result.is_ok())
        .with_context(|| format!("recording the {product} release run for '{distro_id}'"));
    match (build_result, recorded) {
        (Ok(()), Ok(_)) => {
            runs::prune(&bundle.repo_root, distro_id, &release_dir, runs::RELEASE_RETENTION)?;
            Ok(())
        }
        (Ok(()), Err(record)) => Err(record),
        (Err(build), Ok(_)) => Err(build),
        (Err(build), Err(record)) => Err(build.context(format!("and the run record failed too: {record:#}"))),
    }
}
