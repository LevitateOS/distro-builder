use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
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

/// Where the outputs of `flavour`'s preseed are recorded in the variant's
/// Recipe work directory: the base lock's, and the desktop lock's.
fn preseed_record(flavour: Flavour) -> &'static str {
    match flavour {
        Flavour::Base => "rootfs-source-inputs.json",
        Flavour::Desktop => "desktop-source-inputs.json",
    }
}

/// Where the outputs of the apps-build lock's preseed are recorded.
const APPS_BUILD_RECORD: &str = "apps-build-source-inputs.json";

/// Run an explicitly declared acquisition recipe (the preseed of `flavour`'s
/// lock) and validate selected products. Materialization never calls this
/// implicitly.
pub fn preseed_rootfs_source(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
    flavour: Flavour,
    refresh: bool,
) -> Result<BTreeMap<String, PathBuf>> {
    preseed(repo_root, distro_id, recipe_script, preseed_record(flavour), refresh)
}

/// The same for the apps-build lock: what compiling the desktop apps needs
/// beyond the base. It goes with the desktop flavour's acquisition.
pub fn preseed_apps_build(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
    refresh: bool,
) -> Result<BTreeMap<String, PathBuf>> {
    preseed(repo_root, distro_id, recipe_script, APPS_BUILD_RECORD, refresh)
}

fn preseed(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
    record: &str,
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
        Some(&repo_root.join("distro/builder/os-recipes")),
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
    save_preseed_outputs(&build_dir.join(record), &recipe_path, &outputs)?;
    Ok(outputs)
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PreseedOutputs {
    recipe: PathBuf,
    outputs: BTreeMap<String, PathBuf>,
}

fn save_preseed_outputs(
    path: &Path,
    recipe: &Path,
    outputs: &BTreeMap<String, PathBuf>,
) -> Result<()> {
    let work = path.parent().context("the preseed record has no directory")?;
    let record = PreseedOutputs {
        recipe: recipe.canonicalize()?,
        outputs: outputs.clone(),
    };
    let mut file = tempfile::NamedTempFile::new_in(work)?;
    serde_json::to_writer(file.as_file_mut(), &record)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    std::fs::File::open(work)?.sync_all()?;
    Ok(())
}

/// The recorded outputs of `flavour`'s preseed (`recipe`), which the rootfs
/// recipe takes as inputs.
pub(crate) fn preseed_inputs(
    repo: &Path,
    distro: &str,
    recipe: &Path,
    flavour: Flavour,
) -> Result<BTreeMap<String, PathBuf>> {
    recorded_inputs(repo, distro, recipe, preseed_record(flavour), &flavour.to_string())
}

/// The recorded outputs of the apps-build lock's preseed (`recipe`), which
/// the apps-build root recipe takes as inputs.
pub(crate) fn apps_build_inputs(repo: &Path, distro: &str, recipe: &Path) -> Result<BTreeMap<String, PathBuf>> {
    recorded_inputs(repo, distro, recipe, APPS_BUILD_RECORD, "apps-build")
}

fn recorded_inputs(
    repo: &Path,
    distro: &str,
    recipe: &Path,
    record: &str,
    flavour: &str,
) -> Result<BTreeMap<String, PathBuf>> {
    let required = levitate_recipe::needs::declaration(
        recipe,
        Some(&repo.join("distro/builder/os-recipes")),
    )?
    .needs
    .outputs;
    let record_path = recipe_work_dir(repo, distro)?.join(record);
    let bytes = std::fs::read(&record_path).with_context(|| {
        format!(
            "no declared {flavour} rootfs source inputs at {}; run explicit preseed acquisition for the {flavour} lock",
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
