use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

pub(crate) fn locate_repo_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        let candidate = Path::new(ancestor);
        if candidate.join("xtask").is_dir() && candidate.join("distro-variants").is_dir() {
            return Ok(candidate.to_path_buf());
        }
    }
    bail!(
        "unable to locate repository root from '{}' for policy guard",
        manifest_dir.display()
    )
}
