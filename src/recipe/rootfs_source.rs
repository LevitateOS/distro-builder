use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{
    find_recipe, recipe_work_dir, run_recipe_phase_json_with_defines,
    run_recipe_phase_json_with_defines_and_env,
};

#[derive(Debug, Clone)]
pub struct RootfsSourceRecipeSpec {
    pub recipe_script: PathBuf,
    pub defines: BTreeMap<String, String>,
}

fn resolve_recipe_path(repo_root: &Path, recipe_script: &Path) -> PathBuf {
    if recipe_script.is_absolute() {
        recipe_script.to_path_buf()
    } else {
        repo_root.join(recipe_script)
    }
}

pub fn materialize_rootfs_from_recipe(
    repo_root: &Path,
    build_dir: &Path,
    spec: &RootfsSourceRecipeSpec,
) -> Result<PathBuf> {
    let recipe_path = resolve_recipe_path(repo_root, &spec.recipe_script);
    if !recipe_path.is_file() {
        bail!(
            "rootfs source recipe script not found: '{}'",
            recipe_path.display()
        );
    }

    let recipe_bin = find_recipe(repo_root).context("resolving recipe binary for rootfs source")?;
    let recipes_path = recipe_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "rootfs source recipe has no parent directory: '{}'",
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
            "materializing rootfs source via '{}'",
            recipe_path.display()
        )
    })?;

    super::outputs::directory(&ctx, "rootfs").with_context(|| {
        format!(
            "rootfs source recipe '{}' output contract",
            recipe_path.display()
        )
    })
}

/// Run an explicitly declared acquisition recipe and validate selected products.
/// Materialization never calls this implicitly.
pub fn preseed_rootfs_source(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
    required_outputs: &BTreeMap<String, distro_contract::RecipeOutputKind>,
    refresh: bool,
) -> Result<BTreeMap<String, PathBuf>> {
    if required_outputs.is_empty() {
        bail!("preseed requires explicit named output contracts");
    }
    let recipe_path = resolve_recipe_path(repo_root, recipe_script);
    if !recipe_path.is_file() {
        bail!(
            "rootfs source preseed recipe script not found: '{}'",
            recipe_path.display()
        );
    }
    let build_dir = recipe_work_dir(repo_root, distro_id)?;
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating rootfs source preseed work directory '{}'",
            build_dir.display()
        )
    })?;
    let recipe_bin =
        find_recipe(repo_root).context("resolving recipe binary for rootfs source preseed")?;
    let envs = if refresh {
        vec![("ROOTFS_SOURCE_FORCE_REFRESH", "1")]
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
        recipe_path.parent(),
    )
    .with_context(|| format!("preseeding rootfs source via '{}'", recipe_path.display()))?;
    required_outputs
        .iter()
        .map(|(name, kind)| {
            let path = match kind {
                distro_contract::RecipeOutputKind::File => super::outputs::file(&ctx, name),
                distro_contract::RecipeOutputKind::Directory => {
                    super::outputs::directory(&ctx, name)
                }
            }?;
            Ok((name.clone(), path))
        })
        .collect::<Result<_>>()
        .with_context(|| format!("preseed recipe '{}' output contract", recipe_path.display()))
}

/// Compatibility convenience for callers explicitly requesting the DVD ISO.
pub fn preseed_rootfs_source_dvd(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
    refresh: bool,
) -> Result<PathBuf> {
    preseed_rootfs_source(
        repo_root,
        distro_id,
        recipe_script,
        &BTreeMap::from([("iso".into(), distro_contract::RecipeOutputKind::File)]),
        refresh,
    )?
    .remove("iso")
    .context("preseed did not report its iso output")
}
