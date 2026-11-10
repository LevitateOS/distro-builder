use anyhow::{Context, Result};
use std::path::Path;

use distro_builder::product::ProductId;

use distro_contract::runs;

/// Build the releases `product` is built on that are missing or were built
/// from older sources than the checkout's.
pub(crate) fn ensure_release_prerequisites(
    repo_root: &Path,
    distro_id: &str,
    product: ProductId,
) -> Result<()> {
    let variant = distro_contract::load(repo_root, distro_id)?;
    let parents =
        distro_builder::parent_releases_to_build(repo_root, distro_id, &variant.contract, product)?;
    for parent in parents {
        println!(
            "[release:iso:{product}:{distro_id}] building parent release '{parent}' (missing, or built from older sources)..."
        );
        build_one(repo_root, distro_id, parent)?;
    }

    Ok(())
}

pub(crate) fn build_one(repo_root: &Path, distro_id: &str, product: ProductId) -> Result<()> {
    let bundle = distro_contract::load(repo_root, distro_id)?;

    let kernel_output_dir = runs::kernel_dir(&bundle.repo_root, distro_id);
    let mut run = runs::Run::start_release(&bundle.repo_root, distro_id, product)?;
    let iso_path = run.dir.join(bundle.contract.artifacts.iso(product));
    run.manifest.iso_path = Some(iso_path.clone());
    run.save()?;

    let build_result = (|| -> Result<()> {
        // Release builds never build a kernel.
        distro_builder::kernel::check_kernel_installed(&bundle, &kernel_output_dir).with_context(|| {
            format!("install it first: cargo xtask build kernel {distro_id} (--prebuilt downloads it)")
        })?;
        println!("[release:iso:{product}:{distro_id}] kernel already installed");
        distro_builder::release::assemble(&bundle, product, &run.dir, &kernel_output_dir)?;

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
