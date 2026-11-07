use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{find_recipe, recipe_work_dir};

#[derive(Debug, Clone)]
pub struct RootfsSourceRecipeSpec {
    pub storage_max_objects: Option<u64>,
    pub recipe_script: PathBuf,
    pub defines: BTreeMap<String, String>,
    pub inputs: BTreeMap<String, PathBuf>,
    pub network_hosts: Vec<String>,
    pub memory_mib: Option<u64>,
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

    let inputs: Vec<(&str, &Path)> = spec
        .inputs
        .iter()
        .map(|(name, path)| (name.as_str(), path.as_path()))
        .collect();
    let ctx = super::run_recipe_phase_json_impl(
        &recipe_bin.path,
        "install",
        &recipe_path,
        build_dir,
        &defines,
        Some(recipes_path),
        &inputs,
        &spec.network_hosts,
        spec.memory_mib,
        false,
        spec.storage_max_objects,
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
    network_hosts: &[String],
    memory_mib: Option<u64>,
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
    let ctx = super::run_recipe_phase_json_impl(
        &recipe_bin.path,
        "install",
        &recipe_path,
        &build_dir,
        &[],
        recipe_path.parent(),
        &[],
        network_hosts,
        memory_mib,
        refresh,
        None,
    )
    .with_context(|| format!("preseeding rootfs source via '{}'", recipe_path.display()))?;
    let outputs: BTreeMap<String, PathBuf> = required_outputs
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
        .with_context(|| format!("preseed recipe '{}' output contract", recipe_path.display()))?;
    save_preseed_outputs(&build_dir, &recipe_path, &outputs)?;
    Ok(outputs)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PreseedOutputs {
    recipe: PathBuf,
    outputs: BTreeMap<String, PathBuf>,
}

fn save_preseed_outputs(
    work: &Path,
    recipe: &Path,
    outputs: &BTreeMap<String, PathBuf>,
) -> Result<()> {
    let record = PreseedOutputs {
        recipe: recipe.canonicalize()?,
        outputs: outputs.clone(),
    };
    let mut file = tempfile::NamedTempFile::new_in(work)?;
    serde_json::to_writer(file.as_file_mut(), &record)?;
    file.as_file().sync_all()?;
    file.persist(work.join("rootfs-source-inputs.json"))?;
    std::fs::File::open(work)?.sync_all()?;
    Ok(())
}

pub(crate) fn preseed_inputs(
    repo: &Path,
    distro: &str,
    recipe: &Path,
    required: &BTreeMap<String, distro_contract::RecipeOutputKind>,
) -> Result<BTreeMap<String, PathBuf>> {
    let record_path = recipe_work_dir(repo, distro)?.join("rootfs-source-inputs.json");
    let bytes = std::fs::read(&record_path).with_context(|| {
        format!(
            "no declared rootfs source inputs at {}; run explicit preseed acquisition",
            record_path.display()
        )
    })?;
    let record: PreseedOutputs = serde_json::from_slice(&bytes)?;
    if record.recipe != recipe.canonicalize()? {
        bail!("rootfs preseed inputs belong to a different recipe");
    }
    for (name, kind) in required {
        let path = record
            .outputs
            .get(name)
            .with_context(|| format!("preseed output {name} is missing"))?;
        let valid = match kind {
            distro_contract::RecipeOutputKind::File => path.is_file(),
            distro_contract::RecipeOutputKind::Directory => path.is_dir(),
        };
        if !valid {
            bail!(
                "preseed output {name} is unavailable or has the wrong kind: {}",
                path.display()
            );
        }
    }
    Ok(record.outputs)
}
