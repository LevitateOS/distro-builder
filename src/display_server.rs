//! The display server a desktop system takes from source instead of from a
//! package: the contract's `desktop.display_server_recipe_script`. AcornOS names
//! XLibre (`os-recipes/xlibre.rhai`), which Alpine does not package; LevitateOS
//! names none and gets its display server from its packages.
//!
//! It is made the way the desktop apps are (`crate::desktop_apps`): the recipe
//! runs in the variant's apps-build root with the static Recipe as its worker,
//! its `rootfs` tree is cached as the prebuilt family `display-server`
//! (`crate::prebuilt`: local cache, else a download, else a compile that fills
//! the cache) and merged into the desktop system's rootfs with its target
//! metadata, which carries the setuid bit of the server's wrapper.

use anyhow::{bail, Context, Result};
use distro_contract::Variant;
use std::path::{Path, PathBuf};

use crate::pipeline::paths::resolve_repo_path;

/// The server binary, the setuid wrapper and the link the display manager
/// starts (SDDM runs `/usr/bin/X`, which runs the wrapper script `Xorg`).
const SERVER: &str = "usr/libexec/Xorg";
const WRAPPER: &str = "usr/libexec/Xorg.wrap";
const LINK: &str = "usr/bin/X";
/// The modules XLibre loads first: only modules built against its own SDK.
const MODULES: &str = "usr/lib/xorg/modules/xlibre-25";
/// The drivers the desktop cannot run without: the video driver built into
/// the server and the input driver built against it.
const DRIVERS: [&str; 2] = ["drivers/modesetting_drv.so", "input/libinput_drv.so"];

/// The variant's display server recipe, if it compiles one.
pub(crate) fn recipe(variant: &Variant) -> Option<PathBuf> {
    let script = variant.contract.desktop.display_server_recipe_script.as_deref()?;
    Some(resolve_repo_path(&variant.repo_root, script))
}

/// Puts the display server for `variant` into its desktop system `rootfs`, when the
/// variant compiles one; prebuilt when there is a prebuilt tree, else compiled and cached.
pub(crate) fn install_system(variant: &Variant, rootfs: &Path) -> Result<()> {
    if recipe(variant).is_none() {
        return Ok(());
    }
    let tree = obtain_tree(variant)?;
    levitate_recipe::target::merge_tree(&tree, rootfs)
        .with_context(|| format!("merging the display server into '{}'", rootfs.display()))?;
    check(variant, rootfs)
}

fn obtain_tree(variant: &Variant) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let step = crate::timeline::Step::begin("prebuilt", &format!("display-server {}", variant.id));
    if !crate::recipe::exact_identity() {
        step.key(crate::prebuilt::display_server_key(repo, variant)?.short());
    }
    let tree = match crate::prebuilt::lookup_display_server(repo, variant)? {
        Some(tree) => tree,
        None => {
            let compiled = compile_tree(variant)?;
            crate::prebuilt::store_display_server(repo, variant, &compiled)?
        }
    };
    step.finish();
    Ok(tree)
}

/// `prebuilt build display-server <distro>`: compiles the display server into the local
/// cache, with no download and no ISO, after acquiring the locks its build root is made of.
pub(crate) fn build_cache(variant: &Variant) -> Result<()> {
    if recipe(variant).is_none() {
        bail!("{} compiles no display server (no desktop.display_server_recipe_script)", variant.id);
    }
    crate::desktop_apps::acquire_build_locks(variant)?;
    let tree = compile_tree(variant)?;
    crate::prebuilt::store_display_server(&variant.repo_root, variant, &tree)?;
    Ok(())
}

fn compile_tree(variant: &Variant) -> Result<PathBuf> {
    let recipe = recipe(variant).context("no display server recipe")?;
    let work = variant.repo_root.join(".artifacts/work/display-server").join(&variant.id);
    crate::desktop_apps::compile_in_build_root(variant, &recipe, &work, &[])
        .with_context(|| format!("compiling the display server through '{}'", recipe.display()))
}

/// Refuses a desktop rootfs of a variant that compiles its display server when it lacks
/// the server, the setuid root wrapper (by its target metadata), `/usr/bin/X`, the
/// modesetting driver or the libinput driver. Nothing to check for other variants.
pub(crate) fn check(variant: &Variant, rootfs: &Path) -> Result<()> {
    if recipe(variant).is_none() {
        return Ok(());
    }
    let mut files = vec![SERVER.to_string(), WRAPPER.to_string()];
    files.extend(DRIVERS.iter().map(|driver| format!("{MODULES}/{driver}")));
    for path in &files {
        if !rootfs.join(path).is_file() {
            bail!("the desktop system lacks /{path} (the display server)");
        }
    }
    if std::fs::symlink_metadata(rootfs.join(LINK)).is_err() {
        bail!("the desktop system lacks /{LINK}, which the display manager starts");
    }
    let manifest = levitate_recipe::target::load(rootfs)
        .with_context(|| format!("reading the target metadata of '{}'", rootfs.display()))?;
    match manifest.entries.get(WRAPPER) {
        Some(entry) if entry.mode & 0o4000 != 0 && entry.uid == 0 => Ok(()),
        Some(entry) => bail!(
            "the desktop system's /{WRAPPER} is not setuid root (mode {:o}, owner {})",
            entry.mode,
            entry.uid
        ),
        None => bail!("the desktop system's /{WRAPPER} has no target metadata, so it would not be setuid root"),
    }
}
