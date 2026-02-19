use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{find_recipe, run_recipe_phase_json_with_defines};

#[derive(Debug, Clone)]
pub struct RockyStage01RecipeSpec {
    pub recipe_script: PathBuf,
    pub iso_name: String,
    pub sha256: String,
    pub sha256_url: String,
    pub torrent_url: String,
    pub preseed_iso_path: PathBuf,
    pub trust_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Stage01RootfsRecipeSpec {
    pub recipe_script: PathBuf,
    pub defines: BTreeMap<String, String>,
}

pub fn materialize_rootfs_from_recipe(
    repo_root: &Path,
    build_dir: &Path,
    spec: &Stage01RootfsRecipeSpec,
) -> Result<PathBuf> {
    let recipe_path = if spec.recipe_script.is_absolute() {
        spec.recipe_script.clone()
    } else {
        repo_root.join(&spec.recipe_script)
    };
    if !recipe_path.is_file() {
        bail!(
            "Stage 01 rootfs source recipe script not found: '{}'",
            recipe_path.display()
        );
    }

    let recipe_bin =
        find_recipe(repo_root).context("resolving recipe binary for Stage 01 rootfs source")?;
    let recipes_path = recipe_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Stage 01 rootfs recipe has no parent directory: '{}'",
            recipe_path.display()
        )
    })?;

    let mut defines: Vec<(&str, &str)> = Vec::with_capacity(spec.defines.len());
    for (key, value) in &spec.defines {
        defines.push((key.as_str(), value.as_str()));
    }

    let ctx = run_recipe_phase_json_with_defines(
        &recipe_bin.path,
        "install",
        &recipe_path,
        build_dir,
        &defines,
        Some(recipes_path),
    )
    .with_context(|| {
        format!(
            "materializing Stage 01 rootfs source via '{}'",
            recipe_path.display()
        )
    })?;

    let rootfs = ctx["rootfs_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| build_dir.join("rootfs"));

    if !rootfs.is_dir() {
        bail!(
            "Stage 01 rootfs source directory missing after recipe run: '{}'",
            rootfs.display()
        );
    }

    Ok(rootfs)
}

pub fn materialize_rootfs(
    repo_root: &Path,
    build_dir: &Path,
    spec: &RockyStage01RecipeSpec,
) -> Result<PathBuf> {
    let defines = BTreeMap::from([
        ("ROCKY_ISO_NAME".to_string(), spec.iso_name.clone()),
        ("ROCKY_SHA256".to_string(), spec.sha256.clone()),
        ("ROCKY_SHA256_URL".to_string(), spec.sha256_url.clone()),
        ("ROCKY_TORRENT_URL".to_string(), spec.torrent_url.clone()),
        (
            "ROCKY_PRESEED_ISO".to_string(),
            spec.preseed_iso_path.display().to_string(),
        ),
        (
            "ROCKY_TRUST_DIR".to_string(),
            spec.trust_dir.display().to_string(),
        ),
    ]);

    materialize_rootfs_from_recipe(
        repo_root,
        build_dir,
        &Stage01RootfsRecipeSpec {
            recipe_script: spec.recipe_script.clone(),
            defines,
        },
    )
    .with_context(|| "materializing Stage 01 Rocky rootfs source")
}
