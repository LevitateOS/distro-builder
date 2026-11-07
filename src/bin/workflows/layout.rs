use std::path::PathBuf;

use anyhow::{bail, Context, Result};

/// The checkout distro-builder works on: the current directory (xtask and the
/// release hooks run it from the repository root).
pub(crate) fn repo_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("resolving the current directory")?;
    if !cwd.join("distro/variants").is_dir() {
        bail!(
            "'{}' is not a LevitateOS checkout (no distro/variants); run distro-builder from the repository root",
            cwd.display()
        );
    }
    Ok(cwd)
}
