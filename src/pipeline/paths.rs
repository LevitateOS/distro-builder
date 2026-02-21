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
        "iuppiter" | "iuppiteros" => Ok("iuppiter"),
        "ralph" | "ralphos" => Ok("ralph"),
        other => bail!("unsupported distro '{}' for {} resolution", other, purpose),
    }
}
