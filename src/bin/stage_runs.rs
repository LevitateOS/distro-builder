use std::path::{Path, PathBuf};

use anyhow::Result;

pub fn manifest_path(stage_run_dir: &Path) -> PathBuf {
    distro_builder::stage_runs::manifest_path(stage_run_dir)
}

pub fn latest_successful_stage_run_id(stage_root_dir: &Path) -> Result<Option<String>> {
    distro_builder::stage_runs::latest_successful_run_id(stage_root_dir)
}

pub fn prune_old_stage_runs(stage_root_dir: &Path, keep: usize) -> Result<()> {
    distro_builder::stage_runs::prune_old_runs(stage_root_dir, keep)
}

pub fn allocate_stage_run_dir(stage_root_dir: &Path) -> Result<(String, PathBuf)> {
    distro_builder::stage_runs::allocate_run_dir(stage_root_dir)
}
