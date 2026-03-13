use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

use super::{find_recipe, run_recipe_phase_json_with_defines_and_env};
use crate::pipeline::paths::normalize_distro_id;

pub const ALPINE_STAGE01_RECIPE_FILENAME: &str = "alpine-stage01-rootfs.rhai";

#[derive(Debug, Clone)]
pub struct AlpineStage01PreseedOutput {
    pub iso_path: PathBuf,
    pub apk_tools_path: PathBuf,
}

pub fn is_alpine_stage01_recipe(recipe_script: &Path) -> bool {
    recipe_script
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == ALPINE_STAGE01_RECIPE_FILENAME)
        .unwrap_or(false)
}

pub fn preseed_alpine_stage01_assets(
    repo_root: &Path,
    distro_id: &str,
    refresh: bool,
) -> Result<AlpineStage01PreseedOutput> {
    let build_dir = downloads_work_dir(repo_root, distro_id)?;
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating Alpine preseed work directory '{}'",
            build_dir.display()
        )
    })?;

    let recipe_path = repo_root.join("distro-builder/recipes/alpine-preseed-stage01-assets.rhai");
    if !recipe_path.is_file() {
        bail!(
            "Alpine preseed recipe script not found: '{}'",
            recipe_path.display()
        );
    }

    let recipe_bin =
        find_recipe(repo_root).context("resolving recipe binary for Alpine Stage 01 preseed")?;
    let recipes_path = recipe_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Alpine preseed recipe has no parent directory: '{}'",
            recipe_path.display()
        )
    })?;

    let envs = if refresh {
        vec![("ALPINE_FORCE_REFRESH", "1")]
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
    .with_context(|| {
        format!(
            "preseeding Alpine Stage 01 assets via '{}'",
            recipe_path.display()
        )
    })?;

    let iso_path = ctx["iso_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| build_dir.join("alpine-extended-3.23.2-x86_64.iso"));
    let apk_tools_path = ctx["apk_tools_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| build_dir.join("apk-tools-static-3.0.5-r0.apk"));

    if !iso_path.is_file() {
        bail!(
            "Alpine preseed recipe did not produce ISO at '{}'",
            iso_path.display()
        );
    }
    if !apk_tools_path.is_file() {
        bail!(
            "Alpine preseed recipe did not produce apk-tools package at '{}'",
            apk_tools_path.display()
        );
    }

    Ok(AlpineStage01PreseedOutput {
        iso_path,
        apk_tools_path,
    })
}

fn downloads_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "Stage 01 Alpine preseed")?;
    Ok(repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads"))
}
