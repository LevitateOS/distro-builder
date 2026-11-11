//! The system rootfs source: the tree made from the variant's packages (the
//! unpack store and composition, `crate::compose`; the package-effects
//! recipe) and configured by its rootfs recipe, with the identity and setup
//! program the builder gives it, for either flavour: the base system from the
//! base lock, or the desktop system from the base and desktop locks.
use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{ConformanceContract, Variant};
use distro_profile::PackageBackend;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::compose;
use crate::pipeline::paths::resolve_repo_path;
use crate::recipe::rootfs_source::{
    lock_rows, materialize_rootfs_from_recipe, preseed_inputs, LockRow, RootfsSourceRecipeSpec,
};
use crate::recipe::Invocation;

/// The `flavour` system's `/etc/os-release`: its identity and distro profile,
/// from the contract, and its flavour. The system recipe writes it
/// (`installed-system.rhai`), and reads the flavour from it. `LOGO` names the
/// icon the desktop system installs the variant's mark as
/// (`distributor-logo-<os_id>`); `HOME_URL` is there when the OS has a website.
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
    let home_url = identity
        .home_url
        .as_deref()
        .map(|url| format!("HOME_URL={}\n", quoted(url)))
        .unwrap_or_default();
    format!(
        "NAME={}\nID={}\nVERSION_ID={}\nVERSION={}\nPRETTY_NAME={}\nLOGO=distributor-logo-{}\n{}{}{}",
        quoted(&identity.os_name),
        identity.os_id,
        quoted(&identity.os_version),
        quoted(&identity.os_version),
        quoted(&identity.os_name),
        identity.os_id,
        home_url,
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

/// The preseed recipe that downloads the apps-build lock (what compiling the
/// desktop apps needs beyond the base), and the recipe that makes the root
/// they are compiled in.
pub fn apps_build_preseed_recipe(variant: &Variant) -> PathBuf {
    resolve_repo_path(&variant.repo_root, &variant.contract.desktop.apps_build_preseed_recipe_script)
}

pub fn apps_build_root_recipe(variant: &Variant) -> PathBuf {
    resolve_repo_path(&variant.repo_root, &variant.contract.desktop.apps_build_root_recipe_script)
}

/// Acquires the package locks the `flavour` system build reads, before it reads
/// their recorded inputs: the base lock; for the desktop also the desktop lock and
/// the apps-build lock (what compiling the desktop apps needs). Recipe's readiness
/// checks make a preseed whose lock and recipe are unchanged a quick no-op; only
/// `artifact preseed-rootfs-source --refresh` re-acquires.
pub fn acquire_locks(variant: &Variant, flavour: Flavour) -> Result<()> {
    use crate::recipe::rootfs_source::{preseed_apps_build, preseed_rootfs_source};
    let (repo, id) = (&variant.repo_root, &variant.id);
    let mut flavours = vec![Flavour::Base];
    if flavour == Flavour::Desktop {
        flavours.push(Flavour::Desktop);
    }
    for lock in flavours {
        preseed_rootfs_source(repo, id, &preseed_recipe(variant, lock), lock, false)
            .with_context(|| format!("acquiring the {lock} package lock for '{id}'"))?;
    }
    if flavour == Flavour::Desktop {
        preseed_apps_build(repo, id, &apps_build_preseed_recipe(variant), false)
            .with_context(|| format!("acquiring the apps-build package lock for '{id}'"))?;
    }
    Ok(())
}

/// The package-effects step's recipe for each package backend: the composed
/// packages with what their scripts would have done.
const RPM_EFFECTS_RECIPE: &str = "distro/builder/os-recipes/fedora-rpm-package-effects.rhai";
const APK_EFFECTS_RECIPE: &str = "distro/builder/os-recipes/alpine-apk-package-effects.rhai";

/// The `flavour` system tree, in four steps, each cached by what determines it:
///
/// 1. **packages**: LevitateOS unpacks the locked RPMs the unpack store lacks
///    (`fedora-rpm-unpack.rhai`, in shards at once; `crate::compose`);
///    AcornOS installs its base lock's APKs (`alpine-apk-base-tree.rhai`).
/// 2. **compose** (LevitateOS): the base lock's packages composed on the
///    host, and for the desktop the desktop lock's over that tree.
/// 3. **package effects** (`<backend>-package-effects.rhai`): the composed
///    tree (AcornOS: the base tree, with the desktop lock installed over it
///    for the desktop) with what the packages' scripts would have done.
///    Its inputs are the packages and the flavour only.
/// 4. **system configuration** (the variant's rootfs recipe, over
///    `rootfs-producer.rhai`): that tree configured as the installable
///    system, and the capability check. Returns the tree it publishes.
///
/// A configuration change reruns step 4 only. Each flavour has its own work
/// directories, so building one never invalidates the other.
pub fn materialize_rootfs_source(variant: &Variant, flavour: Flavour) -> Result<PathBuf> {
    acquire_locks(variant, flavour)?;
    let repo = &variant.repo_root;
    let id = &variant.id;
    let source = &variant.contract.sources.rootfs_source;
    let recipe = resolve_repo_path(repo, &source.recipe_script);
    let work = distro_contract::runs::rootfs_source_work_dir(repo, id);
    let suffix = match flavour {
        Flavour::Base => String::new(),
        Flavour::Desktop => format!("-{flavour}"),
    };
    let rendered = work.join(format!("builder-inputs{suffix}"));
    fs::create_dir_all(&rendered)
        .with_context(|| format!("creating {}", rendered.display()))?;
    let mut locks = vec![Flavour::Base];
    if flavour == Flavour::Desktop {
        locks.push(Flavour::Desktop);
    }
    let seq = crate::timeline::Seq::new();
    let step = |what: &str| {
        crate::tprintln!("[rootfs:{id}:{flavour}] {what}");
        seq.step(what);
    };

    // 1 and 2: the locked packages as one tree.
    let mut effects_inputs = BTreeMap::new();
    let (effects_recipe, view) = match variant.contract.profile.packages {
        PackageBackend::Rpm => {
            step("unpacking the locked packages (fedora-rpm-unpack.rhai)");
            let mut layers = Vec::new();
            let mut preseed_dirs = Vec::new();
            for lock in &locks {
                let preseed = preseed_recipe(variant, *lock);
                layers.push(lock_rows(&preseed)?);
                preseed_dirs.extend(preseed_inputs(repo, id, &preseed, *lock)?.into_values());
            }
            let rows: Vec<LockRow> = layers.concat();
            let packages = compose::package_files(repo, &rows, &preseed_dirs)?;
            let unpacked = compose::unpack(repo, &packages)
                .with_context(|| format!("unpacking the {id} {flavour} packages"))?;
            step("composing the system tree");
            let (base_layer, desktop_layer) = unpacked.split_at(layers[0].len());
            let base = compose::compose(repo, None, base_layer)?;
            let composed = match flavour {
                Flavour::Base => base,
                Flavour::Desktop => compose::compose(repo, Some(&base), desktop_layer)?,
            };
            (RPM_EFFECTS_RECIPE, compose::input_view(repo, &composed)?)
        }
        PackageBackend::Apk => {
            step("installing the base tree (alpine-apk-base-tree.rhai)");
            for lock in &locks {
                for (name, path) in preseed_inputs(repo, id, &preseed_recipe(variant, *lock), *lock)? {
                    add_input(&mut effects_inputs, &name, path)?;
                }
            }
            let apks = effects_inputs
                .get("apks")
                .context("the base preseed publishes no apks directory")?
                .clone();
            let archive = compose::apk_base_tree(repo, id, &apks)?;
            let rows = lock_rows(&preseed_recipe(variant, Flavour::Base))?;
            (APK_EFFECTS_RECIPE, compose::archive_view(repo, &archive, &rows)?)
        }
    };

    // 3: the packages' effects.
    let effects_recipe = repo.join(effects_recipe);
    step(&format!("package effects ({})", file_name(&effects_recipe)?));
    add_input(&mut effects_inputs, "composed", view.path().to_path_buf())?;
    let flavour_file = rendered.join("flavour");
    fs::write(&flavour_file, format!("{flavour}\n"))
        .with_context(|| format!("writing {}", flavour_file.display()))?;
    add_input(&mut effects_inputs, "flavour", flavour_file)?;
    let mut invocation = Invocation::new(
        repo,
        "install",
        &effects_recipe,
        work.join(format!("{}{suffix}", stem(&effects_recipe)?)),
    );
    for (name, path) in &effects_inputs {
        invocation = invocation.input(name, path);
    }
    let system = invocation
        .run()
        .with_context(|| format!("applying the {id} {flavour} packages' effects"))?
        .file("tree")
        .with_context(|| format!("package-effects recipe '{}' output contract", effects_recipe.display()))?;
    drop(view);

    // 4: the system configuration.
    step(&format!("system configuration ({})", file_name(&recipe)?));
    let mut inputs = BTreeMap::new();
    add_input(&mut inputs, "system", system)?;
    // What the system recipe installs from the builder: the setup program
    // and the system's identity; the desktop system also starts its
    // display manager and shows the variant's branding.
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
    if flavour == Flavour::Desktop {
        for (name, file) in distro_contract::BRANDING {
            add_input(&mut inputs, name, variant.path(file))?;
        }
    }
    let build_dir = work.join(format!("{}{suffix}", stem(&recipe)?));
    fs::create_dir_all(&build_dir)
        .with_context(|| format!("creating {}", build_dir.display()))?;
    let rootfs = materialize_rootfs_from_recipe(
        repo,
        &build_dir,
        &RootfsSourceRecipeSpec {
            recipe_script: recipe,
            inputs,
        },
    )
    .with_context(|| format!("materializing the {id} {flavour} rootfs source"))?;
    seq.finish();
    Ok(rootfs)
}

/// A recipe's file stem, for its work directory.
fn stem(recipe: &Path) -> Result<&str> {
    recipe
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .with_context(|| format!("recipe '{}' has no file name", recipe.display()))
}

fn file_name(recipe: &Path) -> Result<&str> {
    recipe
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("recipe '{}' has no file name", recipe.display()))
}
