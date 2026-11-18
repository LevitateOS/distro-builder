//! Products and their prepared directories. Every product is the installable
//! base system plus an overlay, as `crate::product::build` says. Both are
//! parts (`crate::parts`): the one system part `system` is built here by
//! `prepare_system` (the rootfs recipe's tree, its payload role and the rootfs
//! tools), and the overlay part `overlay-<product>` by `build_overlay` on that
//! system's tree. Every product shares the system part, so test-installed-boot
//! boots exactly the rootfs every ISO installs; the release medium's overlay
//! adds the default software set, which recsetup installs through Recipe
//! (`crate::default_set`). A prepared directory
//! (`.prepared-product.json`, read by `PreparedProduct::read`) lays out one
//! product's rootfs and overlay trees from its parts.
use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{ProductId, Variant};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::io::create_empty_overlay_dir;
use crate::pipeline::live_tools::{add_tools, ToolLayer};
use crate::pipeline::overlay::{create_live_overlay, LiveSession};
use crate::pipeline::source::{materialize_rootfs_source, os_release};
use crate::pipeline::sources;

/// The rootfs tree's directory in a system part, a prepared product's
/// directory and a release run's directory, beside the overlay tree.
pub(crate) const ROOTFS_SOURCE_DIR: &str = "rootfs-source";
/// The manifest in a prepared product's directory.
const MANIFEST: &str = ".prepared-product.json";
/// The installed system's setup program (the system recipe installs it).
const SETUP_PROGRAM: &str = "usr/libexec/recipe-system-setup";
/// The root effects recipe Recipe runs after a system change (the system
/// recipe installs it from `os-recipes/root-effects.rhai`).
const ROOT_EFFECTS: &str = "usr/lib/recipe/effects.rhai";

/// A prepared product: its rootfs and overlay trees in its directory (in a
/// release run's directory, links to its parts' trees, only ever read), and
/// the images they are packed into there.
#[derive(Debug, Serialize, Deserialize)]
pub struct PreparedProduct {
    pub product: String,
    pub distro_id: String,
    /// The trees, relative to the product's directory.
    rootfs_source_dir: PathBuf,
    overlay_dir: PathBuf,
    pub rootfs_erofs: String,
    pub overlay_erofs: String,
}

impl PreparedProduct {
    /// The product prepared in `dir`. Refuses a manifest whose trees are not
    /// inside `dir`, and a missing tree or an empty rootfs.
    pub fn read(dir: &Path) -> Result<Self> {
        let path = dir.join(MANIFEST);
        let bytes = fs::read(&path)
            .with_context(|| format!("reading the prepared product manifest {}", path.display()))?;
        let prepared: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing the prepared product manifest {}", path.display()))?;
        for (what, tree) in [
            ("rootfs", &prepared.rootfs_source_dir),
            ("overlay", &prepared.overlay_dir),
        ] {
            levitate_recipe::target::relative(tree)
                .with_context(|| format!("{}: the {what} tree must be inside the product's directory", path.display()))?;
            if !dir.join(tree).is_dir() {
                bail!("{} has no {what} tree {}", dir.display(), tree.display());
            }
        }
        let rootfs = prepared.rootfs_source(dir);
        if fs::read_dir(&rootfs)?.next().is_none() {
            bail!("the prepared rootfs {} is empty", rootfs.display());
        }
        Ok(prepared)
    }

    /// The rootfs tree of the product prepared in `dir`.
    pub fn rootfs_source(&self, dir: &Path) -> PathBuf {
        dir.join(&self.rootfs_source_dir)
    }

    /// The overlay tree of the product prepared in `dir`.
    pub fn overlay(&self, dir: &Path) -> PathBuf {
        dir.join(&self.overlay_dir)
    }
}

/// Prepares `product` of `variant` in `dir`, with the kernel in `kernel`
/// (`runs::kernel_dir`): ensures its system and overlay parts and copies
/// their trees into `dir` (so `transform build product-erofs` may finalize
/// and pack them there without touching the parts), with the system's
/// `system-sources.sha256` and the manifest.
pub fn prepare_product(
    variant: &Variant,
    product: ProductId,
    dir: &Path,
    kernel: &Path,
) -> Result<PreparedProduct> {
    let build = crate::product::build(product);
    if build.install_screen {
        crate::install_ux::check(&variant.repo_root)
            .with_context(|| format!("resolving the install screen's apps for {}", variant.id))?;
    }
    let system = crate::parts::system(variant, kernel)?;
    let overlay = crate::parts::overlay(variant, product, &system)?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let rootfs = create_empty_overlay_dir(dir, ROOTFS_SOURCE_DIR)?;
    levitate_recipe::target::merge_tree(&system.tree(), &rootfs)
        .with_context(|| format!("copying the system part's tree into {}", rootfs.display()))?;
    let overlay_tree = create_empty_overlay_dir(dir, build.overlay_dir)?;
    // An empty overlay (test-base-rootfs, test-installed-boot) stays empty, metadata too.
    if fs::read_dir(overlay.tree())?.next().is_some() {
        levitate_recipe::target::merge_tree(&overlay.tree(), &overlay_tree)
            .with_context(|| format!("copying the overlay part's tree into {}", overlay_tree.display()))?;
    }
    let digest = sources::recorded(&system.dir)
        .with_context(|| format!("the system part {} records no sources", system.dir.display()))?;
    sources::record(dir, &digest)?;
    write_manifest(variant, product, dir)
}

/// Writes the manifest of `product` prepared in `dir`, whose rootfs tree is
/// `dir/rootfs-source` and overlay tree `dir/<overlay_dir>` (directories, or
/// in a release run's directory links to its parts' trees).
pub(crate) fn write_manifest(variant: &Variant, product: ProductId, dir: &Path) -> Result<PreparedProduct> {
    let contract = &variant.contract;
    let prepared = PreparedProduct {
        product: product.name().to_string(),
        distro_id: variant.id.clone(),
        rootfs_source_dir: PathBuf::from(ROOTFS_SOURCE_DIR),
        overlay_dir: PathBuf::from(crate::product::build(product).overlay_dir),
        rootfs_erofs: contract.artifacts.rootfs.clone(),
        overlay_erofs: contract.artifacts.overlay.clone(),
    };
    let manifest = dir.join(MANIFEST);
    distro_contract::runs::write_atomic(&manifest, &serde_json::to_vec_pretty(&prepared)?)
        .with_context(|| format!("writing {}", manifest.display()))?;
    PreparedProduct::read(dir)
}

/// Writes `product`'s overlay into `overlay` (an empty directory) for the
/// system tree `rootfs`, which it only reads: the live session with the
/// `network` group, for a product with the install screen the overlay tools
/// and the screen, and on the release medium (the desktop flavour) the recab
/// app for A/B installs, the Updater for mutable installs and the default
/// software set's recipes and list. Its package cache is not in the overlay:
/// the release parts (`parts-<category>.tar.zst`) are written into `parts_dir`
/// (the overlay part's directory, where they are recorded outputs).
pub(crate) fn build_overlay(variant: &Variant, product: ProductId, rootfs: &Path, overlay: &Path, parts_dir: &Path) -> Result<()> {
    let contract = &variant.contract;
    let build = crate::product::build(product);
    if build.live_session {
        create_live_overlay(variant, &LiveSession::new(variant, product), rootfs, overlay)?;
        crate::default_set::add_live_network(variant, rootfs, overlay)
            .with_context(|| format!("adding the live session's network to {}", variant.id))?;
    }
    if build.install_screen {
        add_tools(variant, overlay, ToolLayer::Overlay)
            .with_context(|| format!("adding the live tools for {}", variant.id))?;
        crate::install_ux::write_payload(
            &variant.repo_root,
            rootfs,
            overlay,
            &contract.identity.os_name,
            &contract.profile,
            &contract.look,
        )
        .with_context(|| format!("writing the install screen for {}", variant.id))?;
        if product.flavour() == Flavour::Desktop {
            crate::desktop_apps::install_mode_only(variant, overlay)
                .with_context(|| format!("adding the recab app and the Updater to the live medium of {}", variant.id))?;
            crate::default_set::write_medium(variant, overlay, parts_dir)
                .with_context(|| format!("adding the default software set to the live medium of {}", variant.id))?;
        }
    }
    check_live_only(overlay, rootfs)
}

/// The installable base system in `rootfs`: the rootfs recipe's tree, marked
/// as the live init's rootfs layer (recinit's payload-role protocol), with
/// the rootfs tools and the release's group recipes
/// (`crate::update_release::add_release_recipes`).
pub(crate) fn prepare_system(variant: &Variant, rootfs: &Path) -> Result<()> {
    let source = materialize_rootfs_source(variant)?;
    levitate_recipe::target::merge_tree(&source, rootfs)?;
    let role = rootfs.join(recinit::live::PAYLOAD_ROLE_FILE);
    fs::write(&role, format!("{}\n", recinit::live::ROLE_ROOTFS))
        .with_context(|| format!("writing {}", role.display()))?;
    add_tools(variant, rootfs, ToolLayer::Rootfs)?;
    crate::update_release::add_release_recipes(variant, rootfs)
        .with_context(|| format!("adding the release's group recipes to the {} system", variant.id))?;
    check_installable(rootfs, variant).with_context(|| {
        format!(
            "the rootfs recipe's tree {} predates the current system recipe; remove it to rebuild it",
            source.display()
        )
    })
}

/// Refuses a rootfs that is not the current installable system: one without
/// the setup program or the root effects recipe, or whose `/etc/os-release`
/// is not the contract's.
fn check_installable(root: &Path, variant: &Variant) -> Result<()> {
    let contract = &variant.contract;
    for program in [SETUP_PROGRAM, ROOT_EFFECTS] {
        if !root.join(program).is_file() {
            bail!("{} has no /{program}", root.display());
        }
    }
    let path = root.join("etc/os-release");
    let found = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if found != os_release(contract) {
        bail!("{} is not the contract's identity and profile of the system", path.display());
    }
    Ok(())
}

/// The overlay is the only place of what it writes: refuses a non-empty
/// regular file of `overlay` that `rootfs`, the system below it, also has
/// with the same content (a system tree that already carries the live
/// session).
fn check_live_only(overlay: &Path, rootfs: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(overlay).follow_links(false) {
        let entry = entry?;
        let len = entry.metadata()?.len();
        if !entry.file_type().is_file() || len == 0 {
            continue;
        }
        let relative = entry.path().strip_prefix(overlay)?;
        let below = rootfs.join(relative);
        let same = match fs::symlink_metadata(&below) {
            Ok(meta) if meta.is_file() && meta.len() == len => {
                fs::read(&below)? == fs::read(entry.path())?
            }
            Ok(_) => false,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                false
            }
            Err(error) => return Err(error).with_context(|| format!("reading {}", below.display())),
        };
        if same {
            bail!(
                "the live layer's {} is also in the system tree {}; the system recipe must not carry it",
                relative.display(),
                rootfs.display()
            );
        }
    }
    Ok(())
}
