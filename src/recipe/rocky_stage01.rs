use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{find_recipe, run_recipe_phase_json_with_defines, run_recipe_phase_json_with_defines_and_env};
use crate::pipeline::paths::normalize_distro_id;

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

pub fn preseed_rocky_iso(repo_root: &Path, distro_id: &str, refresh: bool) -> Result<PathBuf> {
    let build_dir = downloads_work_dir(repo_root, distro_id)?;
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating Rocky preseed work directory '{}'",
            build_dir.display()
        )
    })?;

    let recipe_path = repo_root.join("distro-builder/recipes/rocky-preseed-iso.rhai");
    if !recipe_path.is_file() {
        bail!(
            "Rocky preseed recipe script not found: '{}'",
            recipe_path.display()
        );
    }

    let recipe_bin = find_recipe(repo_root).context("resolving recipe binary for Rocky preseed")?;
    let recipes_path = recipe_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Rocky preseed recipe has no parent directory: '{}'",
            recipe_path.display()
        )
    })?;

    let envs = if refresh {
        vec![("ROCKY_FORCE_REFRESH", "1")]
    } else {
        Vec::new()
    };

    let ctx = run_recipe_phase_json_with_defines_and_env(
        &recipe_bin.path,
        "install",
        &recipe_path,
        &build_dir,
        &[],
        &envs,
        Some(recipes_path),
    )
    .with_context(|| format!("preseeding Rocky ISO via '{}'", recipe_path.display()))?;

    let iso_path = ctx["iso_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| build_dir.join("Rocky-10.1-x86_64-dvd1.iso"));

    if !iso_path.is_file() {
        bail!(
            "Rocky preseed recipe did not produce ISO at '{}'",
            iso_path.display()
        );
    }

    Ok(iso_path)
}

fn downloads_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "Stage 01 Rocky preseed")?;
    Ok(repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads"))
}
