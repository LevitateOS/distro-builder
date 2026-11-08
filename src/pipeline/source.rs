//! The system rootfs source: the tree the variant's rootfs recipe makes from
//! its packages, with the identity and setup program the builder gives it.
use anyhow::{bail, Context, Result};
use distro_contract::{ConformanceContract, Variant};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::pipeline::paths::resolve_repo_path;
use crate::recipe::rootfs_source::{materialize_rootfs_from_recipe, RootfsSourceRecipeSpec};

/// The system's `/etc/os-release`: its identity and distro profile, from the
/// contract. The system recipe writes it (`installed-system.rhai`).
pub(crate) fn os_release(contract: &ConformanceContract) -> String {
    let identity = &contract.identity;
    let quoted = |value: &str| {
        let mut text = String::from("\"");
        for c in value.chars() {
            if matches!(c, '"' | '\\' | '$' | '`') {
                text.push('\\');
            }
            text.push(c);
        }
        text + "\""
    };
    format!(
        "NAME={}\nID={}\nVERSION_ID={}\nVERSION={}\nPRETTY_NAME={}\n{}",
        quoted(&identity.os_name),
        identity.os_id,
        quoted(&identity.os_version),
        quoted(&identity.os_version),
        quoted(&identity.os_name),
        contract.profile.os_release_lines(),
    )
}

/// Adds input `name`; the variant, the builder and the preseed each name
/// their own.
fn add_input(inputs: &mut BTreeMap<String, PathBuf>, name: &str, path: PathBuf) -> Result<()> {
    if inputs.insert(name.to_string(), path).is_some() {
        bail!("the rootfs recipe's input {name} is given twice");
    }
    Ok(())
}

/// Runs the variant's rootfs recipe and returns the tree it publishes: the
/// whole installable system.
pub fn materialize_rootfs_source(variant: &Variant) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let source = &variant.contract.sources.rootfs_source;
    let recipe = resolve_repo_path(repo, &source.recipe_script);
    let work = distro_contract::runs::rootfs_source_work_dir(repo, &variant.id);

    let mut inputs = BTreeMap::new();
    for (name, path) in &source.inputs {
        add_input(&mut inputs, name, resolve_repo_path(repo, path))?;
    }
    // What the system recipe installs from the builder: the setup program
    // and the system's identity.
    let rendered = work.join("builder-inputs");
    fs::create_dir_all(&rendered)
        .with_context(|| format!("creating {}", rendered.display()))?;
    for (name, text) in [
        ("system_setup", crate::guest::render(&crate::guest::SYSTEM_SETUP, &[])?),
        ("os_release", os_release(&variant.contract)),
    ] {
        let path = rendered.join(name);
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        add_input(&mut inputs, name, path)?;
    }
    if let Some(preseed) = &source.preseed_recipe_script {
        for (name, path) in crate::recipe::rootfs_source::preseed_inputs(
            repo,
            &variant.id,
            &resolve_repo_path(repo, preseed),
            &source.preseed_outputs,
        )? {
            add_input(&mut inputs, &name, path)?;
        }
    }

    let stem = recipe
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("rootfs source recipe '{}' has no file name", recipe.display()))?;
    let build_dir = work.join(stem);
    fs::create_dir_all(&build_dir)
        .with_context(|| format!("creating {}", build_dir.display()))?;
    materialize_rootfs_from_recipe(
        repo,
        &build_dir,
        &RootfsSourceRecipeSpec {
            storage_max_objects: source.storage_max_objects,
            recipe_script: recipe,
            defines: source.defines.clone(),
            network_hosts: source.network_hosts.clone(),
            memory_mib: source.memory_mib,
            inputs,
        },
    )
    .with_context(|| format!("materializing the {} rootfs source", variant.id))
}
