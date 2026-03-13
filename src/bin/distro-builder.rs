use anyhow::{Context, Result};
use serde::Serialize;

mod stage_paths;
mod stage_run_manifest;
mod stage_runs;
mod workflows;

const PRODUCT_BASE_ROOTFS: &str = "base-rootfs";
const PRODUCT_LIVE_BOOT: &str = "live-boot";
const PRODUCT_LIVE_TOOLS: &str = "live-tools";
const DEFAULT_DISTRO_ID: &str = "levitate";
const S00_RUN_RETENTION_COUNT: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuildStage {
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
    rootfs_erofs_filename: &'static str,
    overlay_erofs_filename: &'static str,
    initramfs_live_filename: &'static str,
    live_overlay_dir_name: &'static str,
    rootfs_source_pointer_filename: &'static str,
    issue_banner_label: &'static str,
    compatibility_stage: BuildStage,
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
    compatibility_stage_name: String,
    compatibility_stage_slug: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
    root_dir: String,
    output_dir: String,
    iso_path: String,
}

fn usage() -> &'static str {
    "Usage:\n  distro-builder release build iso [<distro_id|product>] [<distro_id|product>]\n    product defaults to base-rootfs, distro defaults to levitate\n    products: base-rootfs | live-boot | live-tools\n    compatibility aliases: 00Build|01Boot|02LiveTools|0|00|1|01|2|02\n  distro-builder release build-all iso [base-rootfs|live-boot|live-tools]\n  distro-builder product prepare <base-rootfs|live-boot|live-tools> <distro_id> <output_dir>\n  distro-builder transform build rootfs-erofs <source_dir> <output>\n  distro-builder transform build overlayfs-erofs <source_dir> <output>\n  distro-builder transform build product-erofs <prepared_product_dir>\n  distro-builder artifact preseed-stage01-source <distro_id> [--refresh]\n  distro-builder artifact materialize-stage01-source-rootfs <distro_id>\n\nCompatibility aliases:\n  distro-builder iso build [<distro_id|stage>] [<distro_id|stage>]\n  distro-builder iso build-all [00Build|01Boot|02LiveTools]\n  distro-builder artifact build-stage-erofs <stage> <distro_id>\n  distro-builder artifact prepare-stage-inputs <stage> <distro_id> <output_dir>\n  distro-builder artifact prepare-s00-build-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s01-boot-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s02-live-tools-inputs <distro_id> <output_dir>"
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
