//! The desktop apps (`tools/gui/`): Recipe, Recdirstat, the recab app and the Updater.
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
//! 4. That tree is published as the prebuilt `desktop-apps-<key>`
//!    (`prebuilt publish desktop-apps`), which the `rec-apps` group recipe pins:
//!    Recipe and Recdirstat reach a system through Recipe, never through an
//!    image. The recab app ([`AB_DESKTOP_APP_FILES`]), which only an A/B system
//!    gets, and the Updater ([`MUTABLE_DESKTOP_APP_FILES`]), which only a mutable
//!    system gets, are the exceptions: the release medium carries them in
//!    recsetup's A/B-only and mutable-only trees ([`install_tools::AB_ONLY_TREE`],
//!    [`install_tools::MUTABLE_ONLY_TREE`]), and the boot step of that kind
//!    copies its tree into the target.
//!
//! Steps 1-3 are skipped when a prebuilt tree exists (`crate::prebuilt`, family
//! `desktop-apps`): the local cache, else a download. A compile fills the cache.
//!
//! Rust selects the source, the root and the image paths; the recipes own
//! unpacking, compiling and publishing. The build root is never in an image.

use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{Variant, AB_DESKTOP_APP_FILES, MUTABLE_DESKTOP_APP_FILES};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::source::{apps_build_preseed_recipe, apps_build_root_recipe, preseed_recipe};
use crate::process::Cmd;
use crate::recipe::{rootfs_source, Invocation};

/// Puts the apps that only one kind of installation gets into the live
/// `overlay`, with the metadata the compiled tree records for them: the recab
/// app's files ([`AB_DESKTOP_APP_FILES`]) below recsetup's A/B-only tree and
/// the Updater's ([`MUTABLE_DESKTOP_APP_FILES`]) below its mutable-only tree,
/// which recsetup's boot step of that kind copies into the target.
pub(crate) fn install_mode_only(variant: &Variant, overlay: &Path) -> Result<()> {
    let tree = obtain_tree(variant)?;
    check_mode_files(&tree)?;
    let incoming = levitate_recipe::target::load(&tree)?;
    let mut manifest = levitate_recipe::target::load(overlay)?;
    for (only, files) in [
        (install_tools::AB_ONLY_TREE, AB_DESKTOP_APP_FILES),
        (install_tools::MUTABLE_ONLY_TREE, MUTABLE_DESKTOP_APP_FILES),
    ] {
        let prefix = only.trim_start_matches('/');
        for file in files {
            let dest = overlay.join(prefix).join(file);
            fs::create_dir_all(dest.parent().context("an app file has a parent")?)?;
            fs::copy(tree.join(file), &dest).with_context(|| format!("copying /{file} into the live medium"))?;
            let entry = incoming
                .entries
                .get(*file)
                .cloned()
                .with_context(|| format!("the desktop apps' tree records no metadata for /{file}"))?;
            manifest.entries.insert(format!("{prefix}/{file}"), entry);
        }
    }
    levitate_recipe::target::save(overlay, &manifest)
}

/// Refuses a desktop apps' tree whose recab app files are not exactly
/// [`AB_DESKTOP_APP_FILES`] or whose Updater files are not exactly
/// [`MUTABLE_DESKTOP_APP_FILES`]: a recab file a list misses would reach a
/// mutable system, an Updater file an A/B system.
fn check_mode_files(tree: &Path) -> Result<()> {
    for (app, word, files, list, only) in [
        ("the recab app", "recab", AB_DESKTOP_APP_FILES, "AB_DESKTOP_APP_FILES", "an A/B"),
        ("the Updater app", "updater", MUTABLE_DESKTOP_APP_FILES, "MUTABLE_DESKTOP_APP_FILES", "a mutable"),
    ] {
        for file in files {
            if !tree.join(file).is_file() {
                bail!("the desktop apps' tree lacks /{file} ({app})");
            }
        }
        for entry in walkdir::WalkDir::new(tree).follow_links(false) {
            let entry = entry?;
            let relative = entry.path().strip_prefix(tree)?.to_string_lossy().into_owned();
            if !entry.file_type().is_dir() && relative.contains(word) && !files.contains(&relative.as_str()) {
                bail!(
                    "the desktop apps' tree has /{relative}, a file of {app} that {list} \
                     (distro/contract) does not list; only {only} system may get it"
                );
            }
        }
    }
    Ok(())
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
/// and no ISO. The package locks it reads are acquired by [`build_root`], as on an ISO build.
pub(crate) fn build_cache(variant: &Variant) -> Result<()> {
    refuse_exact_cache_fill(variant)?;
    let tree = compile_tree(variant)?;
    crate::prebuilt::store_desktop_apps(&variant.repo_root, variant, &tree)?;
    Ok(())
}

/// A `prebuilt build` fills the local cache, which an exact (signed) build never does.
pub(crate) fn refuse_exact_cache_fill(variant: &Variant) -> Result<()> {
    if crate::recipe::exact_identity() {
        bail!("an exact (signed) build never fills the prebuilt cache of '{}'; unset the signing key", variant.id);
    }
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
        &[],
    )
    .context("compiling the desktop apps through Recipe")
}

/// Runs the OS recipe `recipe` in `variant`'s apps-build root (made first, see [`build_root`]),
/// with the static Recipe as its worker, keeping its state under `work`, and returns the tree
/// it publishes as its output `rootfs`, with `inputs` imported and `defines` set. The
/// desktop apps, the display server (`crate::display_server`) and the login screen
/// (`crate::login_screen`) are compiled this way.
pub(crate) fn compile_in_build_root(
    variant: &Variant,
    recipe: &Path,
    work: &Path,
    inputs: &[(&str, &Path)],
    defines: &[(&str, &str)],
) -> Result<PathBuf> {
    let repo = &variant.repo_root;
    let (reference, layout) = build_root(variant).context("making the apps-build root")?;
    // The static Recipe the image itself ships: it runs in any root. Only the
    // program, not the packaged tree, which ships the runtime images too.
    let worker = crate::recipe::tools::compile_crate(repo, "levitate-recipe")
        .context("building the static Recipe that runs inside the apps-build root")?
        .join("recipe");
    fs::create_dir_all(work).with_context(|| format!("creating '{}'", work.display()))?;
    let mut invocation = Invocation::new(repo, "install", recipe, work.join("recipe"));
    for (name, path) in inputs {
        invocation = invocation.input(name, path);
    }
    for (name, value) in defines {
        invocation = invocation.define(name, value);
    }
    invocation.root(reference, layout).worker(worker).discard_superseded().run()?.directory("rootfs")
}

/// The version of how the builder makes the apps-build root image ([`build_root`]):
/// which lock rows an RPM root takes (`compose::apps_build_wanted`), how they are
/// unpacked and composed (`compose::unpack`, `compose::compose`, `compose::input_view`)
/// and the tar flags and exclusions of the image's layer. It is in the key of every part
/// compiled in that root (`prebuilt::build_root_fields`). Bump it whenever a change there
/// can change the root those parts see; a bump republishes them all.
/// 2: the root is a one-layer OCI image layout without `etc/hosts` (Recipe injects it).
pub(crate) const APPS_BUILD_ROOT_FORMAT: u32 = 2;

/// The variant's apps-build root as a Ginux image: its reference
/// (`local/NAME@sha256:<manifest digest>`) and its layout tar
/// (`recipe::root_layout`).
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
    // Both locks are acquired here, before they are read: a record left by
    // an older run never stands in for them (an unchanged preseed is reused).
    let base = rootfs_source::preseed_rootfs_source(repo, &variant.id, &preseed_recipe(variant, Flavour::Base), Flavour::Base, false)
        .with_context(|| format!("acquiring the base package lock for '{}'", variant.id))?;
    let apps_build = rootfs_source::preseed_apps_build(repo, &variant.id, &apps_build_preseed_recipe(variant), false)
        .with_context(|| format!("acquiring the apps-build package lock for '{}'", variant.id))?;
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
        let composed = crate::compose::compose(repo, &unpacked)?;
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

    // One tar of the tree, the same bytes for the same tree: the image's
    // one layer, so the same tree always has the same manifest digest.
    // `etc/hosts` stays out: Recipe injects it into every instance and the
    // injector never overwrites a path the image has.
    // A change to these flags changes the root: bump APPS_BUILD_ROOT_FORMAT.
    let layer = work.join("layer.tar");
    let partial = work.join("layer.tar.part");
    Cmd::new("tar")
        .args(["--create", "--format=gnu", "--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner"])
        .arg(format!("--exclude=./{}", levitate_recipe::target::MANIFEST))
        .arg("--exclude=./etc/hosts")
        .arg("--file")
        .arg(&partial)
        .arg("--directory")
        .arg(&tree)
        .arg(".")
        .error_msg(format!("archiving the apps-build root '{}' (GNU tar is needed on the host)", tree.display()))
        .run()?;
    fs::rename(&partial, &layer).with_context(|| format!("publishing '{}'", layer.display()))?;
    crate::recipe::root_layout(repo, &format!("{}-apps-build", variant.id), &layer)
}

/// The files of `tools/gui/` (relative to it) the compile reads, as the CMake files name
/// them: every `CMakeLists.txt`, and of the files git knows of (tracked, or new and not
/// ignored) everything under `kit/` and `icons/` (sources and installed icons) and under
/// each app's `src/` and `data/` (an app is a folder with a `CMakeLists.txt`; `data/`
/// holds the installed menu entries and polkit actions), Markdown files excepted. The
/// stand-ins (`fixtures/`) and READMEs only a checkout uses stay out. The story sources
/// (`*/src/shots.cpp`, `kit/helpstory.*`) are in although `-DLEVITATE_STORIES=OFF`
/// leaves them uncompiled: which files an option drops is CMake's to say.
pub(crate) fn source_files(repo: &Path) -> Result<Vec<PathBuf>> {
    let root = repo.join("tools/gui");
    if !root.join("CMakeLists.txt").is_file() {
        bail!("'{}' has no CMakeLists.txt: the desktop apps' source is missing", root.display());
    }
    // tools/gui/ is a submodule (LevitateOS/desktop-apps): git lists its files from inside it.
    let listing = Cmd::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .error_msg("listing the files of tools/gui/ with git")
        .run()?;
    let listed: Vec<PathBuf> = listing
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        // A tracked file deleted in the worktree is not part of the source.
        .filter(|path| fs::symlink_metadata(root.join(path)).is_ok())
        .collect();
    let apps: std::collections::BTreeSet<&Path> = listed
        .iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "CMakeLists.txt"))
        .filter_map(|path| path.parent())
        .filter(|parent| parent.components().count() == 1)
        .collect();
    let files: Vec<PathBuf> = listed
        .iter()
        .filter(|path| {
            if path.file_name().is_some_and(|name| name == "CMakeLists.txt") {
                return true;
            }
            if path.extension().is_some_and(|extension| extension == "md") {
                return false;
            }
            let parts: Vec<_> = path.components().map(|c| c.as_os_str()).collect();
            match parts.as_slice() {
                [top, _, ..] if *top == "kit" || *top == "icons" => true,
                [app, folder, _, ..] => (*folder == "src" || *folder == "data") && apps.contains(Path::new(app)),
                _ => false,
            }
        })
        .cloned()
        .collect();
    Ok(files)
}

/// A plain copy of the desktop apps' source for the recipe: exactly
/// [`source_files`], with modes made machine-independent
/// (`prebuilt::normalize_modes`). The key hashes this same copy (`prebuilt::gui_digest`).
pub(crate) fn source_snapshot(repo: &Path, work: &Path) -> Result<tempfile::TempDir> {
    let root = repo.join("tools/gui");
    let snapshot = tempfile::Builder::new().prefix("gui-source-").tempdir_in(work)?;
    for file in source_files(repo)? {
        let source = root.join(&file);
        let target = snapshot.path().join(&file);
        fs::create_dir_all(target.parent().context("a source file has a parent")?)?;
        if !fs::symlink_metadata(&source)?.file_type().is_file() {
            bail!("unsupported entry in the desktop apps' source: '{}'", source.display());
        }
        fs::copy(&source, &target).with_context(|| format!("copying '{}'", source.display()))?;
    }
    crate::prebuilt::normalize_modes(snapshot.path())?;
    Ok(snapshot)
}
