use anyhow::{Context, Result};
use serde::Serialize;

mod artifact_paths;
mod run_history;
mod run_manifest;
mod workflows;

const PRODUCT_BASE_ROOTFS: &str = "base-rootfs";
const PRODUCT_LIVE_BOOT: &str = "live-boot";
const PRODUCT_LIVE_TOOLS: &str = "live-tools";
const PRODUCT_INSTALLED_BOOT: &str = "installed-boot";
const DEFAULT_DISTRO_ID: &str = "levitate";
const S00_RUN_RETENTION_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompatibilityBuildStage {
    canonical: &'static str,
    slug: &'static str,
    dir_name: &'static str,
    artifact_tag: &'static str,
    native_build_script: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuildProduct {
    canonical: &'static str,
    release_dir_name: &'static str,
    iso_suffix: &'static str,
    live_overlay_dir_name: &'static str,
    rootfs_source_pointer_filename: &'static str,
    issue_banner_label: &'static str,
}

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

fn usage() -> &'static str {
    "Usage:\n  distro-builder release build iso [<distro_id|product>] [<distro_id|product>]\n    product defaults to base-rootfs, distro defaults to levitate\n    release products: base-rootfs | live-boot | live-tools\n  distro-builder release build-all iso [base-rootfs|live-boot|live-tools]\n  distro-builder product prepare <base-rootfs|live-boot|live-tools|installed-boot> <distro_id> <output_dir>\n  distro-builder transform build rootfs-erofs <source_dir> <output>\n  distro-builder transform build overlayfs-erofs <source_dir> <output>\n  distro-builder transform build product-erofs <prepared_product_dir>\n  distro-builder artifact preseed-rootfs-source <distro_id> [--refresh]\n  distro-builder artifact materialize-rootfs-source <distro_id>"
}

fn main() -> Result<()> {
    arm_parent_death_signal()?;

    let args: Vec<String> = std::env::args().skip(1).collect();

    if workflows::is_release_build_invocation(&args) {
        return workflows::run_release_build_command(&args);
    }

    workflows::enforce_legacy_binding_policy_guard()?;
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
