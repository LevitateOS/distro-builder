//! The desktop apps (`gui/`, [`DESKTOP_APPS`]) in the desktop system.
//!
//! A Qt program fits neither way the other programs are made: the Rust tools
//! are one static binary for every distro and the TUI is compiled by bun per
//! C library, but a Qt program must be compiled and linked against the
//! distro's own Qt and C++ runtime. So it is compiled in the distro's own
//! environment:
//!
//! 1. The variant's apps-build root recipe unpacks the base lock's and the
//!    apps-build lock's packages (compiler, CMake, Qt's development files)
//!    into one tree. The apps-build lock is made in the same run as the
//!    desktop lock, so it names the Qt the desktop ships.
//! 2. That tree becomes a root image: one tar, named by its digest.
//! 3. `desktop-apps.rhai` runs in that root instead of Recipe's usual one,
//!    with the static Recipe binary as its worker (the usual worker is built
//!    for the host's C library, which an Alpine root does not have), and
//!    publishes the tree `cmake --install` writes.
//! 4. That tree is merged into the desktop system's rootfs.
//!
//! Rust selects the source, the root and the image paths; the recipes own
//! unpacking, compiling and publishing. The build root is never in an image.

use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{Variant, DESKTOP_APPS, DESKTOP_APP_ENTRIES};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::pipeline::source::{apps_build_preseed_recipe, apps_build_root_recipe, preseed_recipe};
use crate::process::Cmd;
use crate::recipe::{rootfs_source, Invocation};

/// Compiles the desktop apps for `variant` and puts them into its desktop
/// system `rootfs`: each program in `usr/bin`, its menu entry, and the polkit
/// actions two of them ask the administrator's password through.
pub(crate) fn install_system(variant: &Variant, rootfs: &Path) -> Result<()> {
    let repo = &variant.repo_root;
    let (reference, image) = build_root(variant).context("making the root the desktop apps are compiled in")?;
    // The static Recipe the image itself ships: it runs in any root.
    let worker = crate::recipe::tools::build_tool_tree(repo, "levitate-recipe", None, &variant.contract.profile)
        .context("building the static Recipe that runs inside the apps-build root")?
        .join("usr/bin/recipe");
    let work = repo.join(".artifacts/work/desktop-apps").join(&variant.id);
    fs::create_dir_all(&work).with_context(|| format!("creating '{}'", work.display()))?;
    let source = source_snapshot(repo, &work)?;
    let tree = Invocation::new(
        repo,
        "install",
        repo.join("distro/builder/os-recipes/desktop-apps.rhai"),
        work.join("recipe"),
    )
    .input("source", source.path())
    .root(reference, image)
    .worker(worker)
    .run()
    .context("compiling the desktop apps through Recipe")?
    .directory("rootfs")?;
    levitate_recipe::target::merge_tree(&tree, rootfs)
        .with_context(|| format!("merging the desktop apps into '{}'", rootfs.display()))?;
    check(rootfs)
}

/// Refuses a desktop rootfs that lacks one of the desktop apps or its menu entry.
pub(crate) fn check(rootfs: &Path) -> Result<()> {
    for (app, entry) in DESKTOP_APPS.iter().zip(DESKTOP_APP_ENTRIES) {
        for path in [format!("usr/bin/{app}"), entry.to_string()] {
            if !rootfs.join(&path).is_file() {
                bail!("the desktop system lacks /{path} (the desktop app {app})");
            }
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
    for (name, path) in base.iter().chain(&apps_build) {
        if wanted.contains_key(name) {
            invocation = invocation.input(name, path);
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
        .filter_entry(|entry| entry.file_name() != "fixtures")
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
