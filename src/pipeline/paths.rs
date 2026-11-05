use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub(crate) fn resolve_repo_path(repo_root: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        repo_root.join(candidate)
    }
}

pub(crate) fn normalize_distro_id(distro_id: &str, purpose: &str) -> Result<&'static str> {
    match distro_id.trim().to_ascii_lowercase().as_str() {
        "levitate" | "leviso" => Ok("levitate"),
        "acorn" | "acornos" => Ok("acorn"),
        other => bail!("unsupported distro '{}' for {} resolution", other, purpose),
    }
}

/// Shared acquisition/build work for a distro's recipes, separate from published outputs.
pub fn recipe_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "recipe work directory")?;
    Ok(repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads"))
}
