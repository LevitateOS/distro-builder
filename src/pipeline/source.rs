//! The system rootfs source: the tree the variant's rootfs recipe makes from
//! its packages, with the identity and setup program the builder gives it,
//! for either flavour: the base system from the base lock, or the desktop
//! system from the base and desktop locks.
use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{ConformanceContract, Variant};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::pipeline::paths::resolve_repo_path;
use crate::recipe::rootfs_source::{materialize_rootfs_from_recipe, RootfsSourceRecipeSpec};

/// The `flavour` system's `/etc/os-release`: its identity and distro profile,
/// from the contract, and its flavour. The system recipe writes it
/// (`installed-system.rhai`), and reads the flavour from it.
pub(crate) fn os_release(contract: &ConformanceContract, flavour: Flavour) -> String {
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
        "NAME={}\nID={}\nVERSION_ID={}\nVERSION={}\nPRETTY_NAME={}\n{}{}",
        quoted(&identity.os_name),
        identity.os_id,
        quoted(&identity.os_version),
        quoted(&identity.os_version),
        quoted(&identity.os_name),
        contract.profile.os_release_lines(),
        flavour.os_release_line(),
    )
}

/// Adds input `name`; the builder and the preseed each name their own.
fn add_input(inputs: &mut BTreeMap<String, PathBuf>, name: &str, path: PathBuf) -> Result<()> {
    if inputs.insert(name.to_string(), path).is_some() {
        bail!("the rootfs recipe's input {name} is given twice");
    }
    Ok(())
}

/// The preseed recipe that downloads `flavour`'s lock: the base lock for
/// the base system; the desktop system adds the desktop lock to it.
pub fn preseed_recipe(variant: &Variant, flavour: Flavour) -> PathBuf {
    let contract = &variant.contract;
    resolve_repo_path(
        &variant.repo_root,
        match flavour {
            Flavour::Base => &contract.sources.rootfs_source.preseed_recipe_script,
            Flavour::Desktop => &contract.desktop.preseed_recipe_script,
        },
    )
}

/// Runs the variant's rootfs recipe and returns the tree it publishes: the
/// whole installable `flavour` system. Each flavour has its own build
/// directory, so building one never invalidates the other.
pub fn materialize_rootfs_source(variant: &Variant, flavour: Flavour) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let source = &variant.contract.sources.rootfs_source;
    let recipe = resolve_repo_path(repo, &source.recipe_script);
    let work = distro_contract::runs::rootfs_source_work_dir(repo, &variant.id);
    let suffix = match flavour {
        Flavour::Base => String::new(),
        Flavour::Desktop => format!("-{flavour}"),
    };

    let mut inputs = BTreeMap::new();
    // What the system recipe installs from the builder: the setup program
    // and the system's identity; the desktop system also starts its
    // display manager.
    let rendered = work.join(format!("builder-inputs{suffix}"));
    fs::create_dir_all(&rendered)
        .with_context(|| format!("creating {}", rendered.display()))?;
    let privilege = variant.contract.profile.privilege;
    let mut texts = vec![
        ("system_setup", crate::guest::render(&crate::guest::SYSTEM_SETUP, &[])?),
        ("os_release", os_release(&variant.contract, flavour)),
        ("capabilities", distro_contract::capabilities_text().to_string()),
        ("admin_rule_file", privilege.admin_rule_file().to_string()),
        ("admin_rule", privilege.admin_rule()),
    ];
    if flavour == Flavour::Desktop {
        texts.push(("display_manager", variant.contract.desktop.display_manager.clone()));
    }
    for (name, text) in texts {
        let path = rendered.join(name);
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        add_input(&mut inputs, name, path)?;
    }
    let mut flavours = vec![Flavour::Base];
    if flavour == Flavour::Desktop {
        flavours.push(Flavour::Desktop);
    }
    for lock in flavours {
        for (name, path) in crate::recipe::rootfs_source::preseed_inputs(
            repo,
            &variant.id,
            &preseed_recipe(variant, lock),
            lock,
        )? {
            add_input(&mut inputs, &name, path)?;
        }
    }

    let stem = recipe
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("rootfs source recipe '{}' has no file name", recipe.display()))?;
    let build_dir = work.join(format!("{stem}{suffix}"));
    fs::create_dir_all(&build_dir)
        .with_context(|| format!("creating {}", build_dir.display()))?;
    materialize_rootfs_from_recipe(
        repo,
        &build_dir,
        &RootfsSourceRecipeSpec {
            recipe_script: recipe,
            inputs,
        },
    )
    .with_context(|| format!("materializing the {} {flavour} rootfs source", variant.id))
}
