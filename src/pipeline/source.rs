use anyhow::{Context, Result};

use distro_contract::{
    ConformanceContract, RecipeOutputKind, RootfsSourceContract, RootfsSourceKind,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::paths::resolve_repo_path;
use crate::recipe::rootfs_source::{materialize_rootfs_from_recipe, RootfsSourceRecipeSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RootfsSourcePolicy {
    pub storage_max_objects: Option<u64>,
    pub kind: RootfsSourceKind,
    pub recipe_script: PathBuf,
    pub preseed_recipe_script: Option<PathBuf>,
    pub preseed_outputs: BTreeMap<String, RecipeOutputKind>,
    pub defines: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, PathBuf>,
    pub network_hosts: Vec<String>,
    pub memory_mib: Option<u64>,
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
        storage_max_objects: source.storage_max_objects,
        kind: source.kind,
        recipe_script: resolve_repo_path(repo_root, &source.recipe_script),
        preseed_recipe_script: source
            .preseed_recipe_script
            .as_deref()
            .map(|script| resolve_repo_path(repo_root, script)),
        preseed_outputs: source.preseed_outputs.clone(),
        defines: source.defines.clone(),
        network_hosts: source.network_hosts.clone(),
        memory_mib: source.memory_mib,
        inputs: source
            .inputs
            .iter()
            .map(|(name, path)| (name.clone(), resolve_repo_path(repo_root, path)))
            .collect(),
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
    let mut inputs = source.inputs.clone();
    if let Some(preseed_recipe) = &source.preseed_recipe_script {
        let acquired = crate::recipe::rootfs_source::preseed_inputs(
            repo_root,
            distro_id,
            preseed_recipe,
            &source.preseed_outputs,
        )?;
        for (name, path) in acquired {
            if inputs.insert(name.clone(), path).is_some() {
                anyhow::bail!("duplicate rootfs input {name}");
            }
        }
    }
    let source_rootfs_dir = materialize_rootfs_from_recipe(
        repo_root,
        &build_dir,
        &RootfsSourceRecipeSpec {
            storage_max_objects: source.storage_max_objects,
            recipe_script: source.recipe_script.clone(),
            defines: source.defines.clone(),
            network_hosts: source.network_hosts.clone(),
            memory_mib: source.memory_mib,
            inputs,
        },
    )
    .with_context(|| format!("materializing rootfs source recipe output for '{distro_id}'"))?;
    Ok(source_rootfs_dir)
}

fn rootfs_source_provider_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let provider_dir = distro_contract::runs::rootfs_source_work_dir(repo_root, distro_id);
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
        .with_context(|| format!("rootfs source recipe '{}' has no file name", recipe_script.display()))?;
    Ok(rootfs_source_provider_work_dir(repo_root, distro_id)?.join(recipe_dir_name))
}
