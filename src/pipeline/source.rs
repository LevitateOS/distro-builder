use anyhow::{Context, Result};

use distro_contract::{
    ConformanceContract, RecipeOutputKind, RootfsSourceContract, RootfsSourceKind,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::paths::{normalize_distro_id, resolve_repo_path};
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::recipe::rootfs_source::{materialize_rootfs_from_recipe, RootfsSourceRecipeSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootfsSourcePolicy {
    pub kind: RootfsSourceKind,
    pub recipe_script: PathBuf,
    pub preseed_recipe_script: Option<PathBuf>,
    pub preseed_outputs: BTreeMap<String, RecipeOutputKind>,
    pub defines: BTreeMap<String, String>,
}

pub(crate) fn rootfs_source_policy_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<Option<RootfsSourcePolicy>> {
    Ok(Some(rootfs_source_policy_from_source_contract(
        repo_root,
        &contract.sources.rootfs_source,
    )?))
}

fn rootfs_source_policy_from_source_contract(
    repo_root: &Path,
    source: &RootfsSourceContract,
) -> Result<RootfsSourcePolicy> {
    Ok(RootfsSourcePolicy {
        kind: source.kind,
        recipe_script: resolve_repo_path(repo_root, source.recipe_script.trim()),
        preseed_recipe_script: source
            .preseed_recipe_script
            .as_deref()
            .map(|script| resolve_repo_path(repo_root, script.trim())),
        preseed_outputs: source.preseed_outputs.clone(),
        defines: source.defines.clone(),
    })
}

pub(crate) fn materialize_source_rootfs(
    repo_root: &Path,
    distro_id: &str,
    source_policy: &Option<RootfsSourcePolicy>,
) -> Result<PathBuf> {
    let source = source_policy.as_ref().with_context(|| {
        format!(
        "live-boot product preparation requires a canonical rootfs_source policy for '{distro_id}'")
    })?;
    let build_dir =
        rootfs_source_provider_recipe_work_dir(repo_root, distro_id, &source.recipe_script)?;
    fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating rootfs source recipe provider directory '{}'",
            build_dir.display()
        )
    })?;
    let source_rootfs_dir = materialize_rootfs_from_recipe(
        repo_root,
        &build_dir,
        &RootfsSourceRecipeSpec {
            recipe_script: source.recipe_script.clone(),
            defines: source.defines.clone(),
        },
    )
    .with_context(|| format!("materializing rootfs source recipe output for '{distro_id}'"))?;
    ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
    Ok(source_rootfs_dir)
}

pub(crate) fn cleanup_legacy_provider_dir(output_dir: &Path) -> Result<()> {
    let entries = fs::read_dir(output_dir)
        .with_context(|| format!("reading output directory '{}'", output_dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "reading legacy provider directory entry under '{}'",
                output_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with("-rootfs-provider") {
            continue;
        }
        fs::remove_dir_all(&path)
            .with_context(|| format!("removing legacy provider dir '{}'", path.display()))?;
    }
    Ok(())
}

fn rootfs_source_provider_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "rootfs source provider work directory")?;
    let provider_dir = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("rootfs-source-provider");
    fs::create_dir_all(&provider_dir).with_context(|| {
        format!(
            "creating rootfs source provider work directory '{}'",
            provider_dir.display()
        )
    })?;
    Ok(provider_dir)
}

fn rootfs_source_provider_recipe_work_dir(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
) -> Result<PathBuf> {
    let recipe_dir_name = recipe_script
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("recipe");
    Ok(rootfs_source_provider_work_dir(repo_root, distro_id)?.join(recipe_dir_name))
}
