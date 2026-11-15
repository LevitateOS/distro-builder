//! The display server compiled from source instead of taken from a package:
//! the contract's `desktop.display_server_recipe_script`. AcornOS names XLibre
//! (`os-recipes/xlibre.rhai`), which Alpine does not package; LevitateOS names
//! none.
//!
//! It is made the way the desktop apps are (`crate::desktop_apps`): the recipe
//! runs in the variant's apps-build root with the static Recipe as its worker,
//! and its `rootfs` tree (whose target metadata carries the setuid bit of the
//! server's wrapper) is stored as the prebuilt family `display-server`
//! (`prebuilt build` / `prebuilt publish display-server <distro>`). No image
//! carries it: the server reaches a system only through a group recipe that
//! pins that prebuilt, as `rec-apps` pins the desktop apps'. The `xlibre`
//! group recipe does not pin it yet.

use anyhow::{bail, Context, Result};
use distro_contract::Variant;
use std::path::PathBuf;

use crate::pipeline::paths::resolve_repo_path;

/// The variant's display server recipe, if it compiles one.
pub(crate) fn recipe(variant: &Variant) -> Option<PathBuf> {
    let script = variant.contract.desktop.display_server_recipe_script.as_deref()?;
    Some(resolve_repo_path(&variant.repo_root, script))
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
