//! Alpine Linux dependency via recipe.

use super::find_recipe;
use crate::process::ensure_exists;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Paths produced by the alpine.rhai recipe after execution.
#[derive(Debug, Clone)]
pub struct AlpinePaths {
    /// Path to the downloaded Alpine ISO.
    pub iso: PathBuf,
    /// Path to the extracted rootfs.
    pub rootfs: PathBuf,
}

impl AlpinePaths {
    /// Check if all paths exist.
    pub fn exists(&self) -> bool {
        self.iso.is_file() && self.rootfs.is_dir()
    }
}

/// Run the alpine.rhai recipe and return the output paths.
///
/// # Arguments
/// * `base_dir` - distro crate root (e.g., `/path/to/AcornOS`)
///
/// # Returns
/// The paths to the Alpine artifacts (ISO and rootfs).
pub fn alpine(base_dir: &Path) -> Result<AlpinePaths> {
    let distro_name = super::distro_name(base_dir);
    let monorepo_dir = base_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base_dir.to_path_buf());

    let downloads_dir = base_dir.join("downloads");
    let recipe_path = base_dir.join("deps/alpine.rhai");
    let recipes_path = base_dir.join("deps");

    ensure_exists(&recipe_path, "Alpine recipe").map_err(|_| {
        anyhow::anyhow!(
            "Alpine recipe not found at: {}\n\
             Expected alpine.rhai in {}/deps/",
            recipe_path.display(),
            distro_name
        )
    })?;

    // Find and run recipe, parse JSON output
    // Pass recipes_path so build_deps can find dependency recipes (e.g., 7z-deps.rhai)
    let recipe_bin = find_recipe(&monorepo_dir)?;
    let ctx = super::run_recipe_json_with_defines(
        &recipe_bin.path,
        &recipe_path,
        &downloads_dir,
        &[],
        Some(&recipes_path),
    )?;

    Ok(AlpinePaths {
        iso: super::outputs::file(&ctx, "iso")?,
        rootfs: super::outputs::directory(&ctx, "rootfs")?,
    })
}
