use std::path::{Path, PathBuf};

use anyhow::Result;

#[allow(dead_code)]
pub fn manifest_path(stage_run_dir: &Path) -> PathBuf {
    run_manifest_path(stage_run_dir)
}

#[allow(dead_code)]
pub fn latest_successful_stage_run_id(stage_root_dir: &Path) -> Result<Option<String>> {
    latest_successful_run_id(stage_root_dir)
}

#[allow(dead_code)]
pub fn prune_old_stage_runs(stage_root_dir: &Path, keep: usize) -> Result<()> {
    prune_old_runs(stage_root_dir, keep)
}

#[allow(dead_code)]
pub fn allocate_stage_run_dir(stage_root_dir: &Path) -> Result<(String, PathBuf)> {
    allocate_run_dir(stage_root_dir)
}

pub fn run_manifest_path(run_dir: &Path) -> PathBuf {
    distro_builder::stage_runs::run_manifest_path(run_dir)
}

pub fn latest_successful_run_id(run_root_dir: &Path) -> Result<Option<String>> {
    distro_builder::stage_runs::latest_successful_run_id(run_root_dir)
}

pub fn prune_old_runs(run_root_dir: &Path, keep: usize) -> Result<()> {
    distro_builder::stage_runs::prune_old_runs(run_root_dir, keep)
}

pub fn allocate_run_dir(run_root_dir: &Path) -> Result<(String, PathBuf)> {
    distro_builder::stage_runs::allocate_run_dir(run_root_dir)
}
