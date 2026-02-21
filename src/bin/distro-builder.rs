use anyhow::Result;
use serde::Serialize;

mod stage_paths;
mod stage_run_id;
mod stage_run_manifest;
mod stage_runs;
mod workflows;

const STAGE00_NATIVE_BUILD_SCRIPT: &str = "00Build-build.sh";
const STAGE01_NATIVE_BUILD_SCRIPT: &str = "01Boot-build.sh";
const STAGE02_NATIVE_BUILD_SCRIPT: &str = "02LiveTools-build.sh";
const STAGE00_CANONICAL: &str = "00Build";
const STAGE00_SLUG: &str = "s00_build";
const STAGE00_DIRNAME: &str = "s00-build";
const STAGE00_ARTIFACT_TAG: &str = "s00";
const STAGE01_CANONICAL: &str = "01Boot";
const STAGE01_SLUG: &str = "s01_boot";
const STAGE01_DIRNAME: &str = "s01-boot";
const STAGE01_ARTIFACT_TAG: &str = "s01";
const STAGE02_CANONICAL: &str = "02LiveTools";
const STAGE02_SLUG: &str = "s02_live_tools";
const STAGE02_DIRNAME: &str = "s02-live-tools";
const STAGE02_ARTIFACT_TAG: &str = "s02";
const DEFAULT_DISTRO_ID: &str = "levitate";
const S00_RUN_RETENTION_COUNT: usize = 5;

#[derive(Clone, Copy)]
struct BuildStage {
    canonical: &'static str,
    slug: &'static str,
    dir_name: &'static str,
}

#[derive(Debug, Clone)]
struct StageOutputLayout {
    stage_root_dir: std::path::PathBuf,
    stage_output_dir: std::path::PathBuf,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct StageRunMetadata {
    run_id: String,
    distro_id: String,
    stage_name: String,
    stage_slug: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
    stage_root_dir: String,
    stage_output_dir: String,
    iso_path: String,
}

fn usage() -> &'static str {
    "Usage:\n  distro-builder iso build [<distro_id|stage>] [<distro_id|stage>] \n    stage defaults to 00Build, distro defaults to levitate\n    stage aliases: 0|00|01|1|02|2\n  distro-builder iso build-all [00Build|01Boot|02LiveTools]\n  distro-builder artifact build-rootfs-erofs <source_dir> <output>\n  distro-builder artifact build-overlayfs-erofs <source_dir> <output>\n  distro-builder artifact prepare-stage-inputs <stage> <distro_id> <output_dir>\n  distro-builder artifact prepare-s00-build-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s01-boot-inputs <distro_id> <output_dir>\n  distro-builder artifact prepare-s02-live-tools-inputs <distro_id> <output_dir>"
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if workflows::is_iso_build_invocation(&args) {
        return workflows::run_iso_build_command(&args);
    }

    workflows::enforce_legacy_binding_policy_guard()?;
    workflows::dispatch_non_iso_command(&args)
}
