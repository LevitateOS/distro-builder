use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::stage_run_id;

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
    for _ in 0..32 {
        let run_id = stage_run_id::generate_stage_run_id()?;
        let run_root = stage_root_dir.join(&run_id);
        if run_root.exists() {
            continue;
        }
        fs::create_dir_all(&run_root).with_context(|| {
            format!(
                "creating stage run output directory '{}'",
                run_root.display()
            )
        })?;
        return Ok((run_id, run_root));
    }
    bail!(
        "failed allocating unique stage run directory under '{}'",
        stage_root_dir.display()
    )
}
