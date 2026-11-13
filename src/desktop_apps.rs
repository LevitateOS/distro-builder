//! The desktop apps (`gui/`, [`DESKTOP_APPS`]) in the desktop system.
//!
//! A Qt program fits neither way the other programs are made: the Rust tools
//! are one static binary for every distro and the TUI is compiled by bun per
//! C library, but a Qt program must be compiled and linked against the
//! distro's own Qt and C++ runtime. So it is compiled in the distro's own
//! environment:
//!
//! 1. The variant's apps-build root recipe makes one tree of the base lock's
//!    and the apps-build lock's packages (compiler, CMake, Qt's development
//!    files): on Fedora it copies the tree the builder composed from the
//!    unpack store (`crate::compose`), on Alpine apk installs them. The
//!    apps-build lock is made in the same run as the desktop lock, so it
//!    names the Qt the desktop ships.
//! 2. That tree becomes a root image: one tar, named by its digest.
//! 3. `desktop-apps.rhai` runs in that root instead of Recipe's usual one,
//!    with the static Recipe binary as its worker (the usual worker is built
//!    for the host's C library, which an Alpine root does not have), and
//!    publishes the tree `cmake --install` writes.
//! 4. That tree is merged into the desktop system's rootfs.
//!
//! Steps 1-3 are skipped when a prebuilt tree exists (`crate::prebuilt`, family
//! `desktop-apps`): the local cache, else a download. A compile fills the cache.
//!
//! Rust selects the source, the root and the image paths; the recipes own
//! unpacking, compiling and publishing. The build root is never in an image.

use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{Variant, DESKTOP_APPS, DESKTOP_APP_ENTRIES, DESKTOP_HELPERS};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::pipeline::source::{apps_build_preseed_recipe, apps_build_root_recipe, preseed_recipe};
use crate::process::Cmd;
use crate::recipe::{rootfs_source, Invocation};

/// Puts the desktop apps for `variant` into its desktop system `rootfs`: each app and
/// the helper in `usr/bin`, each app's menu entry, and the polkit actions two of the apps
/// ask the administrator's password through. The tree is a prebuilt one when there is one
/// (never in an exact build, or with `--no-prebuilt`), else it is compiled and cached.
pub(crate) fn install_system(variant: &Variant, rootfs: &Path) -> Result<()> {
    let tree = obtain_tree(variant)?;
    levitate_recipe::target::merge_tree(&tree, rootfs)
        .with_context(|| format!("merging the desktop apps into '{}'", rootfs.display()))?;
    check(rootfs)
}

/// The desktop apps tree: prebuilt (local or downloaded), else compiled and stored.
fn obtain_tree(variant: &Variant) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let step = crate::timeline::Step::begin("prebuilt", &format!("desktop-apps {}", variant.id));
    if !crate::recipe::exact_identity() {
        step.key(crate::prebuilt::desktop_apps_key(repo, variant)?.short());
    }
    let tree = match crate::prebuilt::lookup_desktop_apps(repo, variant)? {
        Some(tree) => tree,
        None => {
            let compiled = compile_tree(variant)?;
            crate::prebuilt::store_desktop_apps(repo, variant, &compiled)?
        }
    };
    step.finish();
    Ok(tree)
}

/// `prebuilt build desktop-apps`: compiles the apps into the local cache, with no download
/// and no ISO. The package locks it reads are acquired first, as an ISO build does.
pub(crate) fn build_cache(variant: &Variant) -> Result<()> {
    acquire_build_locks(variant)?;
    let tree = compile_tree(variant)?;
    crate::prebuilt::store_desktop_apps(&variant.repo_root, variant, &tree)?;
    Ok(())
}

/// Acquires the package locks the apps-build root is made from (the base lock and the
/// apps-build lock), as an ISO build does, for a `prebuilt build` of a tree compiled in it.
pub(crate) fn acquire_build_locks(variant: &Variant) -> Result<()> {
    use crate::recipe::rootfs_source::{preseed_apps_build, preseed_rootfs_source};
    let (repo, id) = (&variant.repo_root, &variant.id);
    if crate::recipe::exact_identity() {
        bail!("an exact (signed) build never fills the prebuilt cache; unset the signing key");
    }
    preseed_rootfs_source(repo, id, &preseed_recipe(variant, Flavour::Base), Flavour::Base, false)
        .with_context(|| format!("acquiring the base package lock for '{id}'"))?;
    preseed_apps_build(repo, id, &apps_build_preseed_recipe(variant), false)
        .with_context(|| format!("acquiring the apps-build package lock for '{id}'"))?;
    Ok(())
}

/// Compiles the desktop apps for `variant` in its apps-build root and returns the tree
/// `cmake --install` wrote (Recipe's output `rootfs`).
fn compile_tree(variant: &Variant) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let work = repo.join(".artifacts/work/desktop-apps").join(&variant.id);
    fs::create_dir_all(&work).with_context(|| format!("creating '{}'", work.display()))?;
    let source = source_snapshot(repo, &work)?;
    compile_in_build_root(
        variant,
        &repo.join("distro/builder/os-recipes/desktop-apps.rhai"),
        &work,
        &[("source", source.path())],
    )
    .context("compiling the desktop apps through Recipe")
}

/// Runs the OS recipe `recipe` in `variant`'s apps-build root (made first, see [`build_root`]),
/// with the static Recipe as its worker, keeping its state under `work`, and returns the tree
/// it publishes as its output `rootfs`. The desktop apps and the display server
/// (`crate::display_server`) are compiled this way.
pub(crate) fn compile_in_build_root(
    variant: &Variant,
    recipe: &Path,
    work: &Path,
    inputs: &[(&str, &Path)],
) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let (reference, image) = build_root(variant).context("making the apps-build root")?;
    // The static Recipe the image itself ships: it runs in any root.
    let worker = crate::recipe::tools::build_tool_tree(repo, "levitate-recipe", None, &variant.contract.profile)
        .context("building the static Recipe that runs inside the apps-build root")?
        .join("usr/bin/recipe");
    fs::create_dir_all(work).with_context(|| format!("creating '{}'", work.display()))?;
    let mut invocation = Invocation::new(repo, "install", recipe, work.join("recipe"));
    for (name, path) in inputs {
        invocation = invocation.input(name, path);
    }
    invocation.root(reference, image).worker(worker).run()?.directory("rootfs")
}

/// Refuses a desktop rootfs that lacks one of the desktop apps, its menu entry
/// or one of the helper programs ([`DESKTOP_HELPERS`], which have no menu entry).
pub(crate) fn check(rootfs: &Path) -> Result<()> {
    for (app, entry) in DESKTOP_APPS.iter().zip(DESKTOP_APP_ENTRIES) {
        for path in [format!("usr/bin/{app}"), entry.to_string()] {
            if !rootfs.join(&path).is_file() {
                bail!("the desktop system lacks /{path} (the desktop app {app})");
            }
        }
    }
    for helper in DESKTOP_HELPERS {
        if !rootfs.join(format!("usr/bin/{helper}")).is_file() {
            bail!("the desktop system lacks /usr/bin/{helper} (the desktop helper)");
        }
    }
    Ok(())
}

/// The variant's apps-build root as a root image: its reference
/// (`oci:NAME@sha256:DIGEST`) and the tar.
fn build_root(variant: &Variant) -> Result<(String, PathBuf)> {
    let repo = &variant.repo_root;
    let recipe = apps_build_root_recipe(variant);
    let work = repo.join(".artifacts/work/apps-build-root").join(&variant.id);
    fs::create_dir_all(&work).with_context(|| format!("creating '{}'", work.display()))?;

    // The root recipe takes the packages of two locks: the base's and the
    // apps-build's. Each preseed may publish more than this recipe reads.
    let wanted = levitate_recipe::needs::declaration(&recipe, Some(&repo.join("distro/builder/os-recipes")))?
        .needs
        .inputs;
    let mut invocation = Invocation::new(repo, "install", &recipe, work.join("recipe"));
    let base = rootfs_source::preseed_inputs(repo, &variant.id, &preseed_recipe(variant, Flavour::Base), Flavour::Base)?;
    let apps_build = rootfs_source::apps_build_inputs(repo, &variant.id, &apps_build_preseed_recipe(variant))?;
    // On an RPM variant the root is composed from the unpack store: the base
    // packages are unpacked once for every tree that has them.
    let mut _view = None;
    if variant.contract.profile.packages == distro_profile::PackageBackend::Rpm {
        let mut rows = rootfs_source::lock_rows(&preseed_recipe(variant, Flavour::Base))?;
        rows.extend(rootfs_source::lock_rows(&apps_build_preseed_recipe(variant))?);
        rows.retain(crate::compose::apps_build_wanted);
        let dirs: Vec<PathBuf> = base.values().chain(apps_build.values()).cloned().collect();
        let packages = crate::compose::package_files(repo, &rows, &dirs)?;
        let unpacked = crate::compose::unpack(repo, &packages).context("unpacking the apps-build root's packages")?;
        let composed = crate::compose::compose(repo, None, &unpacked)?;
        let view = crate::compose::input_view(repo, &composed)?;
        invocation = invocation.input("composed", view.path());
        _view = Some(view);
    } else {
        for (name, path) in base.iter().chain(&apps_build) {
            if wanted.contains_key(name) {
                invocation = invocation.input(name, path);
            }
        }
    }
    let tree = invocation
        .run()
        .with_context(|| format!("unpacking the apps-build root via '{}'", recipe.display()))?
        .directory("root")?;

    // One tar of the tree, the same bytes for the same tree: Recipe checks
    // the digest in the reference against the tar before it runs anything.
    let image = work.join("rootfs.tar");
    let partial = work.join("rootfs.tar.part");
    Cmd::new("tar")
        .args(["--create", "--format=gnu", "--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner"])
        .arg(format!("--exclude=./{}", levitate_recipe::target::MANIFEST))
        .arg("--file")
        .arg(&partial)
        .arg("--directory")
        .arg(&tree)
        .arg(".")
        .error_msg(format!("archiving the apps-build root '{}' (GNU tar is needed on the host)", tree.display()))
        .run()?;
    let digest = sha256_of(&partial)?;
    fs::rename(&partial, &image).with_context(|| format!("publishing '{}'", image.display()))?;
    Ok((format!("oci:{}-apps-build@sha256:{digest}", variant.id), image))
}

fn sha256_of(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).with_context(|| format!("opening '{}'", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).with_context(|| format!("reading '{}'", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// A plain copy of `gui/` for the recipe: the build files, the sources and
/// the data files, without the stand-ins (`fixtures/`), which only a
/// checkout uses.
fn source_snapshot(repo: &Path, work: &Path) -> Result<tempfile::TempDir> {
    let root = repo.join("gui");
    if !root.join("CMakeLists.txt").is_file() {
        bail!("'{}' has no CMakeLists.txt: the desktop apps' source is missing", root.display());
    }
    let snapshot = tempfile::Builder::new().prefix("gui-source-").tempdir_in(work)?;
    for entry in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        // gui/ is a submodule: its `.git` link is the checkout's, not source.
        .filter_entry(|entry| entry.file_name() != "fixtures" && entry.file_name() != ".git")
    {
        let entry = entry?;
        let target = snapshot.path().join(entry.path().strip_prefix(&root)?);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &target)
                .with_context(|| format!("copying '{}'", entry.path().display()))?;
        } else {
            bail!("unsupported entry in the desktop apps' source: '{}'", entry.path().display());
        }
    }
    Ok(snapshot)
}
