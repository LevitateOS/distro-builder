use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{recipe_work_dir, Invocation};

#[derive(Debug, Clone)]
pub struct RootfsSourceRecipeSpec {
    pub recipe_script: PathBuf,
    pub inputs: BTreeMap<String, PathBuf>,
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

    let mut invocation = Invocation::new(repo_root, "install", &recipe_path, build_dir);
    for (name, path) in &spec.inputs {
        invocation = invocation.input(name, path);
    }
    let ctx = invocation.run().with_context(|| {
        format!(
            "materializing rootfs source via '{}'",
            recipe_path.display()
        )
    })?;

    ctx.directory("rootfs").with_context(|| {
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
    refresh: bool,
) -> Result<BTreeMap<String, PathBuf>> {
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
    let required_outputs = levitate_recipe::needs::declaration(
        &recipe_path,
        Some(&repo_root.join("distro/builder/recipes")),
    )?
    .needs
    .outputs;
    if required_outputs.is_empty() {
        bail!("preseed NEEDS.outputs must declare its products");
    }
    let ctx = Invocation::new(repo_root, "install", &recipe_path, &build_dir)
        .force_acquire(refresh)
        .run()
        .with_context(|| format!("preseeding rootfs source via '{}'", recipe_path.display()))?;
    let outputs: BTreeMap<String, PathBuf> = required_outputs
        .iter()
        .map(|(name, kind)| {
            let path = match kind {
                levitate_recipe::needs::PathKind::File => ctx.file(name),
                levitate_recipe::needs::PathKind::Directory => ctx.directory(name),
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
) -> Result<BTreeMap<String, PathBuf>> {
    let required = levitate_recipe::needs::declaration(
        recipe,
        Some(&repo.join("distro/builder/recipes")),
    )?
    .needs
    .outputs;
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
    for (name, kind) in &required {
        let path = record
            .outputs
            .get(name)
            .with_context(|| format!("preseed output {name} is missing"))?;
        let valid = match kind {
            levitate_recipe::needs::PathKind::File => path.is_file(),
            levitate_recipe::needs::PathKind::Directory => path.is_dir(),
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
