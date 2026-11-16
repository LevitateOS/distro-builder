//! The login screen, reclogin (`tools/reclogin`): `reclogind` and `reclogin-greeter`.
//!
//! Unlike the static tools (`crate::recipe::tools`), reclogin links C libraries of
//! the system it runs on: libpam (`reclogind`), libinput and libxkbcommon (the
//! greeter). So it is compiled for each OS the way the desktop apps and the display
//! server are (`crate::desktop_apps::compile_in_build_root`): in that OS's apps-build
//! root, with that OS's own Rust toolchain and the development files of those
//! libraries (`desktop.apps_build_packages`), as dynamically linked programs of the
//! OS's C library. Its tree is stored as the prebuilt family `login-screen`
//! (`prebuilt build` / `prebuilt publish login-screen <distro>`). No image carries
//! it: it reaches a system only through the `login-screen` group recipe
//! (`recipes/login-screen.rhai`), which pins the published release of each OS.
//!
//! The source goes in as an offline closure (`tools::prepare_crate_source`): the
//! crate's files (its `dist/` and `assets/` included), a one-member workspace that
//! inherits the root's package fields and workspace dependencies, the root
//! `Cargo.lock` pruned to the crate, and every crates.io dependency vendored. The
//! recipe (`os-recipes/reclogin.rhai`) compiles it with `cargo build --locked
//! --offline` and declares no network, so a compile reads nothing that is not in
//! the key.

use anyhow::{Context, Result};
use distro_contract::Variant;
use std::path::{Path, PathBuf};

/// The crate, relative to the repository.
pub(crate) const CRATE_DIR: &str = "tools/reclogin";
/// The OS recipe that compiles it in the apps-build root and lays out its tree.
pub(crate) const RECIPE: &str = "distro/builder/os-recipes/reclogin.rhai";

/// `prebuilt build login-screen <distro>`: compiles reclogin for `variant` into the
/// local cache, with no download and no ISO, after acquiring the locks its build root
/// is made of.
pub(crate) fn build_cache(variant: &Variant) -> Result<()> {
    crate::desktop_apps::acquire_build_locks(variant)?;
    let source = prepare_source(&variant.repo_root)?;
    let tree = compile_tree(variant, source.path())?;
    crate::prebuilt::store_login_screen(&variant.repo_root, variant, &tree)?;
    Ok(())
}

/// The offline source closure of the crate (see the module docs).
pub(crate) fn prepare_source(repo: &Path) -> Result<tempfile::TempDir> {
    crate::recipe::tools::prepare_crate_source(repo, Path::new(CRATE_DIR))
}

/// The facts the recipe takes per OS: which brand and PAM file (`OS_ID`, the
/// contract's `identity.os_id`, naming `assets/<os_id>/` and `dist/pam/reclogin.<os_id>`)
/// and which init system's service files (`INIT`).
pub(crate) fn defines(variant: &Variant) -> [(&'static str, String); 2] {
    [
        ("OS_ID", variant.contract.identity.os_id.clone()),
        ("INIT", variant.contract.profile.init.as_str().to_string()),
    ]
}

fn compile_tree(variant: &Variant, source: &Path) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let work = repo.join(".artifacts/work/login-screen").join(&variant.id);
    let defines = defines(variant);
    let defines: Vec<(&str, &str)> = defines.iter().map(|(k, v)| (*k, v.as_str())).collect();
    crate::desktop_apps::compile_in_build_root(variant, &repo.join(RECIPE), &work, &[("source", source)], &defines)
        .with_context(|| format!("compiling the login screen for {} through '{RECIPE}'", variant.id))
}
