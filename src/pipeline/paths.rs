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

/// The Recipe build and state directory of variant `distro_id`
/// (`distro_contract::runs::recipe_work_dir`); an error for an id that names
/// no variant under `distro/variants`.
pub fn recipe_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    if !distro_contract::distro_ids(repo_root).iter().any(|id| id == distro_id) {
        bail!("'{distro_id}' is not a variant under distro/variants");
    }
    Ok(distro_contract::runs::recipe_work_dir(repo_root, distro_id))
}
