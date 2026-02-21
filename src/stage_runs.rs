use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const RUN_MANIFEST_FILENAME: &str = "run-manifest.json";

#[derive(Debug, Deserialize, Clone)]
pub struct RunMetadata {
    pub run_id: String,
    pub status: String,
    pub created_at_utc: String,
    pub finished_at_utc: Option<String>,
}

pub fn manifest_path(run_dir: &Path) -> PathBuf {
    run_dir.join(RUN_MANIFEST_FILENAME)
}

pub fn load_runs_metadata(stage_root_dir: &Path) -> Result<Vec<RunMetadata>> {
    if !stage_root_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(stage_root_dir).with_context(|| {
        format!(
            "reading stage runs directory '{}'",
            stage_root_dir.display()
        )
    })? {
        let entry = entry.with_context(|| {
            format!(
                "iterating stage runs directory '{}'",
                stage_root_dir.display()
            )
        })?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let Some(run_name) = run_dir.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        if run_name.starts_with('.') {
            continue;
        }
        let path = manifest_path(&run_dir);
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("reading stage run metadata '{}'", path.display()))?;
        let parsed: RunMetadata = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing stage run metadata '{}'", path.display()))?;
        runs.push(parsed);
    }
    Ok(runs)
}

pub fn latest_successful_run_id(stage_root_dir: &Path) -> Result<Option<String>> {
    let mut runs = load_runs_metadata(stage_root_dir)?;
    runs.retain(|run| run.status == "success");
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    Ok(runs.first().map(|r| r.run_id.clone()))
}

pub fn prune_old_runs(stage_root_dir: &Path, keep: usize) -> Result<()> {
    let mut runs = load_runs_metadata(stage_root_dir)?;
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    for run in runs.into_iter().skip(keep) {
        let path = stage_root_dir.join(&run.run_id);
        fs::remove_dir_all(&path).with_context(|| {
            format!("removing expired stage run directory '{}'", path.display())
        })?;
    }
    Ok(())
}

fn run_sort_key(run: &RunMetadata) -> String {
    run.finished_at_utc
        .clone()
        .unwrap_or_else(|| run.created_at_utc.clone())
}
