use anyhow::{Context, Result};
use std::path::Path;

use distro_builder::product::ProductId;

use distro_contract::runs;

/// Builds `product`'s release of `distro_id` in a new release run: the
/// combiner (`release::assemble`) ensures the product's system and overlay
/// parts and writes the ISO. No other product's release is built.
pub(crate) fn build_one(repo_root: &Path, distro_id: &str, product: ProductId) -> Result<()> {
    let bundle = distro_contract::load(repo_root, distro_id)?;

    let kernel_output_dir = runs::kernel_dir(&bundle.repo_root, distro_id);
    let mut run = runs::Run::start_release(&bundle.repo_root, distro_id, product)?;
    let iso_path = run.dir.join(bundle.contract.artifacts.iso(product));
    run.manifest.iso_path = Some(iso_path.clone());
    run.save()?;

    distro_builder::timeline::start(&bundle.repo_root, distro_id, product.name(), &run.dir);
    let mut used_parts = Vec::new();
    let build_result = (|| -> Result<()> {
        // Release builds never build a kernel.
        distro_builder::kernel::check_kernel_installed(&bundle, &kernel_output_dir).with_context(|| {
            format!("install it first: cargo xtask build kernel {distro_id} (--prebuilt downloads it)")
        })?;
        distro_builder::tprintln!("[release:iso:{product}:{distro_id}] kernel already installed");
        used_parts = distro_builder::release::assemble(&bundle, product, &run.dir, &kernel_output_dir)?.parts;

        distro_builder::tprintln!(
            "[release:iso:{product}:{distro_id}] built at {}",
            iso_path.display()
        );
        Ok(())
    })();
    distro_builder::timeline::finish(build_result.is_ok());

    let release_dir = runs::release_dir(&bundle.repo_root, distro_id, product);
    let run_dir = run.dir.clone();
    let recorded = run
        .finish(build_result.is_ok())
        .with_context(|| format!("recording the {product} release run for '{distro_id}'"));
    let recorded = recorded.and_then(|finished| {
        hand_back_to_invoking_user(&release_dir, &run_dir)?;
        Ok(finished)
    });
    match (build_result, recorded) {
        (Ok(()), Ok(_)) => {
            runs::prune(&bundle.repo_root, distro_id, &release_dir, runs::RELEASE_RETENTION)?;
            // After the old runs went: a part only they linked goes too.
            if let Err(error) = distro_builder::parts::prune(&bundle.repo_root, distro_id, &used_parts) {
                distro_builder::tprintln!("[parts:{distro_id}] warning: old parts were not pruned: {error:#}");
            }
            Ok(())
        }
        (Ok(()), Err(record)) => Err(record),
        (Err(build), Ok(_)) => Err(build),
        (Err(build), Err(record)) => Err(build.context(format!("and the run record failed too: {record:#}"))),
    }
}

/// Under `sudo` (as `cargo xtask build` runs the builder) the product directory
/// and the run directory are created as root. The user's libvirt session must
/// open the ISO, and with SELinux enforcing it cannot relabel a root-owned one,
/// so hand them to the invoking user (`SUDO_UID`/`SUDO_GID`): the product
/// directory itself, the run directory recursively. Symlinks are re-owned but
/// never followed, so the root-owned part trees they point into stay root's.
/// Does nothing when not root or when sudo's variables are absent.
fn hand_back_to_invoking_user(product_dir: &Path, run_dir: &Path) -> Result<()> {
    use std::os::unix::fs::lchown;

    if unsafe { libc::geteuid() } != 0 {
        return Ok(());
    }
    let id = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u32>().ok());
    let (Some(uid), Some(gid)) = (id("SUDO_UID"), id("SUDO_GID")) else {
        return Ok(());
    };
    let chown = |path: &Path| {
        lchown(path, Some(uid), Some(gid)).with_context(|| format!("handing {} back to uid {uid}", path.display()))
    };
    chown(product_dir)?;
    // walkdir does not follow symlinks by default, so it yields them without descending.
    for entry in walkdir::WalkDir::new(run_dir) {
        let entry = entry.with_context(|| format!("walking {}", run_dir.display()))?;
        chown(entry.path())?;
    }
    Ok(())
}
