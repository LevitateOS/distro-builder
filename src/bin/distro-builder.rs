use anyhow::{Context, Result};
use serde::Serialize;

mod run_manifest;
mod workflows;

const RELEASE_RUN_RETENTION_COUNT: usize = 5;

#[derive(Debug, Clone)]
struct BuildOutputLayout {
    root_dir: std::path::PathBuf,
    output_dir: std::path::PathBuf,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct BuildRunMetadata {
    run_id: String,
    distro_id: String,
    target_kind: String,
    target_name: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
    root_dir: String,
    output_dir: String,
    iso_path: String,
}

fn usage() -> String {
    use distro_contract::ProductId;
    let releases = ProductId::list(ProductId::releases());
    let products = ProductId::list(ProductId::ALL);
    format!(
        "Usage:\n  distro-builder release build iso [<product>] <distro_id>\n    \
         product defaults to base-rootfs; release products: {releases}\n  \
         distro-builder release build-all iso [{releases}]\n  \
         distro-builder product prepare <{products}> <distro_id> <output_dir>\n  \
         distro-builder transform build rootfs-erofs <source_dir> <output>\n  \
         distro-builder transform build overlayfs-erofs <source_dir> <output>\n  \
         distro-builder transform build product-erofs <prepared_product_dir>\n  \
         distro-builder artifact prepare-qemu-tools\n  \
         distro-builder artifact prepare-tool-source <package> <output_dir>\n  \
         distro-builder artifact preseed-rootfs-source <distro_id> [--refresh]\n  \
         distro-builder artifact materialize-rootfs-source <distro_id>"
    )
}

fn main() -> Result<()> {
    arm_parent_death_signal()?;

    let args: Vec<String> = std::env::args().skip(1).collect();

    if workflows::is_release_build_invocation(&args) {
        return workflows::run_release_build_command(&args);
    }

    workflows::dispatch_non_release_command(&args)
}

#[cfg(unix)]
fn arm_parent_death_signal() -> Result<()> {
    // If launcher/wrapper dies (cancel/abort), terminate this process too so
    // long-running sub-steps do not continue as orphans.
    let rc = unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error())
            .context("Failed to arm parent-death signal for distro-builder");
    }
    Ok(())
}

#[cfg(not(unix))]
fn arm_parent_death_signal() -> Result<()> {
    Ok(())
}
