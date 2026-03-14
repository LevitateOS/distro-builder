use std::path::{Path, PathBuf};

use anyhow::Result;

#[allow(dead_code)]
pub fn manifest_path(run_dir: &Path) -> PathBuf {
    run_manifest_path(run_dir)
}

#[allow(dead_code)]
pub fn latest_successful_run_id_at_root(run_root_dir: &Path) -> Result<Option<String>> {
    latest_successful_run_id(run_root_dir)
}

#[allow(dead_code)]
pub fn prune_old_runs_at_root(run_root_dir: &Path, keep: usize) -> Result<()> {
    prune_old_runs(run_root_dir, keep)
}

#[allow(dead_code)]
pub fn allocate_run_dir_at_root(run_root_dir: &Path) -> Result<(String, PathBuf)> {
    allocate_run_dir(run_root_dir)
}

pub fn run_manifest_path(run_dir: &Path) -> PathBuf {
    distro_builder::run_history::run_manifest_path(run_dir)
}

pub fn latest_successful_run_id(run_root_dir: &Path) -> Result<Option<String>> {
    distro_builder::run_history::latest_successful_run_id(run_root_dir)
}

pub fn prune_old_runs(run_root_dir: &Path, keep: usize) -> Result<()> {
    distro_builder::run_history::prune_old_runs(run_root_dir, keep)
}

pub fn allocate_run_dir(run_root_dir: &Path) -> Result<(String, PathBuf)> {
    distro_builder::run_history::allocate_run_dir(run_root_dir)
}
