use std::fs;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

pub fn write_stage_run_metadata<T: Serialize>(path: &Path, metadata: &T) -> Result<()> {
    write_json_atomic(path, metadata)
        .with_context(|| format!("writing stage run metadata '{}'", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path without parent '{}'", path.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("creating parent directory '{}'", parent.display()))?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let payload =
        serde_json::to_vec_pretty(value).with_context(|| "serializing stage run metadata")?;
    fs::write(&tmp, payload).with_context(|| format!("writing temp file '{}'", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| {
        format!(
            "renaming temp file '{}' to '{}'",
            tmp.display(),
            path.display()
        )
    })?;
    Ok(())
}
