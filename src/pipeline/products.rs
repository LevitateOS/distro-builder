//! Product preparation. Every product is the installable system rootfs of
//! its flavour (`ProductId::flavour`) plus a layer: a product without a
//! parent builds the system (the rootfs recipe's tree, its payload role and
//! the rootfs tools): base-rootfs the base system, live-desktop the desktop
//! system. Every other product takes its parent's rootfs image unchanged and
//! adds only its overlay, as `crate::product::build` says. So installed-boot
//! boots exactly the rootfs the live-tools ISO installs, and a desktop
//! install gets exactly the live-desktop ISO's rootfs. One routine prepares
//! every product and records it in one manifest.
use anyhow::{bail, Context, Result};
use distro_contract::profile::Flavour;
use distro_contract::{ConformanceContract, ProductId, Variant};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::io::{create_empty_overlay_dir, extract_erofs_rootfs};
use crate::pipeline::live_tools::{add_tools, ToolLayer};
use crate::pipeline::overlay::{create_live_overlay, LiveSession};
use crate::pipeline::source::{materialize_rootfs_source, os_release};
use crate::pipeline::sources;

/// The prepared product's rootfs tree, beside its overlay tree.
const ROOTFS_SOURCE_DIR: &str = "rootfs-source";
/// The manifest in a prepared product's directory.
const MANIFEST: &str = ".prepared-product.json";
/// The installed system's setup program (the system recipe installs it).
const SETUP_PROGRAM: &str = "usr/libexec/recipe-system-setup";

/// A prepared product: its rootfs and overlay trees in its directory, and the
/// images they are packed into there.
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

/// Prepares `product` of `variant` in `dir`: its rootfs tree, its overlay
/// tree and the manifest.
/// A derived product starts from its parent's latest release.
pub fn prepare_product(variant: &Variant, product: ProductId, dir: &Path) -> Result<PreparedProduct> {
    let contract = &variant.contract;
    let build = crate::product::build(product);
    if build.install_screen {
        crate::install_ux::check(&variant.repo_root)
            .with_context(|| format!("resolving the install screen's apps for {}", variant.id))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let rootfs = create_empty_overlay_dir(dir, ROOTFS_SOURCE_DIR)?;
    let flavour = product.flavour();
    // The sources the system is built from now; a parent's rootfs must have
    // been built from the same.
    let current = sources::system_sources_digest(&variant.repo_root, &variant.id)
        .context("fingerprinting the system's sources")?;
    match product.parent() {
        None => prepare_system(variant, flavour, &rootfs)
            .with_context(|| format!("preparing the {} {flavour} system rootfs", variant.id))?,
        Some(parent) => {
            let image = crate::parent_rootfs_image(&variant.repo_root, &variant.id, contract, product)?
                .with_context(|| format!("{product} has no parent rootfs image"))?;
            let built_from = image.parent().and_then(sources::recorded);
            if built_from.as_deref() != Some(current.as_str()) {
                bail!(
                    "the latest {parent} release ({}) was built from older sources than the checkout's \
                     (its tools, recipes, locks or TUI apps changed since); rebuild {parent}",
                    image.display()
                );
            }
            extract_erofs_rootfs(&image, &rootfs)
                .with_context(|| format!("extracting the {parent} rootfs {}", image.display()))?;
            check_installable(&rootfs, contract, flavour).with_context(|| {
                format!("the {parent} rootfs {} is not the current system; rebuild {parent}", image.display())
            })?;
        }
    }

    let overlay = create_empty_overlay_dir(dir, build.overlay_dir)?;
    if build.live_session {
        create_live_overlay(variant, &LiveSession::new(variant, product), &rootfs, &overlay)?;
    }
    if build.install_screen {
        add_tools(&variant.repo_root, &overlay, ToolLayer::Overlay, &variant.id, &contract.profile)
            .with_context(|| format!("adding the live tools for {}", variant.id))?;
        crate::install_ux::write_payload(
            &variant.repo_root,
            &rootfs,
            &overlay,
            &contract.identity.os_name,
            &contract.profile,
        )
        .with_context(|| format!("writing the install screen for {}", variant.id))?;
    }
    check_live_only(&overlay, &rootfs)?;
    sources::record(dir, &current)?;

    let prepared = PreparedProduct {
        product: product.name().to_string(),
        distro_id: variant.id.clone(),
        rootfs_source_dir: PathBuf::from(ROOTFS_SOURCE_DIR),
        overlay_dir: PathBuf::from(build.overlay_dir),
        rootfs_erofs: contract.artifacts.rootfs.clone(),
        overlay_erofs: contract.artifacts.overlay.clone(),
    };
    let manifest = dir.join(MANIFEST);
    distro_contract::runs::write_atomic(&manifest, &serde_json::to_vec_pretty(&prepared)?)
        .with_context(|| format!("writing {}", manifest.display()))?;
    Ok(prepared)
}

/// The installable `flavour` system in `rootfs`: the rootfs recipe's tree,
/// marked as the live init's rootfs layer (recinit's payload-role protocol),
/// with the rootfs tools.
fn prepare_system(variant: &Variant, flavour: Flavour, rootfs: &Path) -> Result<()> {
    let source = materialize_rootfs_source(variant, flavour)?;
    levitate_recipe::target::merge_tree(&source, rootfs)?;
    let update_source = rootfs.join(distro_contract::release::SOURCE_CONFIG);
    fs::create_dir_all(update_source.parent().unwrap())?;
    fs::write(
        &update_source,
        serde_json::to_vec(&variant.contract.release.installed())?,
    )?;
    if let Some(source) = &variant.contract.release.source {
        let text = fs::read_to_string(variant.repo_root.join(&source.recipe))?;
        let marker = "network: [] /* recab configured endpoint */";
        anyhow::ensure!(
            text.matches(marker).count() == 1,
            "acquisition recipe must declare its configurable network marker exactly once"
        );
        let mut endpoints = std::collections::BTreeSet::from([distro_contract::release::endpoint(&source.index)?]);
        if let Some(production) = &variant.contract.release.production {
            endpoints.insert(distro_contract::release::endpoint(&production.asset_prefix)?);
            for origin in &production.redirect_origins { endpoints.insert(distro_contract::release::endpoint(origin)?); }
            let proof = fs::read(variant.path(distro_contract::release::production::AUTHORITY_FILE))?;
            distro_contract::release::production::AuthorityProof::parse(&proof)?.verify(production,
                &variant.contract.identity.os_id, &variant.contract.release.channel,
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs())?;
            fs::write(rootfs.join("usr/lib/recab/release-keys.json"), proof)?;
            levitate_recipe::target::set_mode(rootfs, Path::new("usr/lib/recab/release-keys.json"), 0o644)?;
        }
        let rendered = text.replace(
            marker,
            &format!("network: {}", serde_json::to_string(&endpoints)?),
        );
        let path = rootfs.join(distro_contract::release::ACQUISITION_RECIPE);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, rendered)?;
        levitate_recipe::target::set_mode(
            rootfs,
            std::path::Path::new(distro_contract::release::ACQUISITION_RECIPE),
            0o644,
        )?;
    }
    let role = rootfs.join(recinit::live::PAYLOAD_ROLE_FILE);
    fs::write(&role, format!("{}\n", recinit::live::ROLE_ROOTFS))
        .with_context(|| format!("writing {}", role.display()))?;
    add_tools(&variant.repo_root, rootfs, ToolLayer::Rootfs, &variant.id, &variant.contract.profile)?;
    crate::tui_binary::install_system(&variant.repo_root, rootfs, &variant.contract.profile)
        .with_context(|| format!("adding the installed system's TUI apps for {}", variant.id))?;
    if flavour == Flavour::Desktop {
        crate::desktop_apps::install_system(variant, rootfs)
            .with_context(|| format!("adding the desktop apps for {}", variant.id))?;
    }
    check_installable(rootfs, &variant.contract, flavour).with_context(|| {
        format!(
            "the rootfs recipe's tree {} predates the current system recipe; remove it to rebuild it",
            source.display()
        )
    })
}

/// Refuses a rootfs that is not the current installable `flavour` system:
/// one without the setup program, or whose `/etc/os-release` is not the
/// contract's for that flavour.
fn check_installable(root: &Path, contract: &ConformanceContract, flavour: Flavour) -> Result<()> {
    if !root.join(SETUP_PROGRAM).is_file() {
        bail!("{} has no setup program /{SETUP_PROGRAM}", root.display());
    }
    let path = root.join("etc/os-release");
    let found = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    if found != os_release(contract, flavour) {
        bail!("{} is not the contract's identity and profile of the {flavour} system", path.display());
    }
    // A desktop system from before the desktop apps existed is not the
    // current one: a parent image without them must be rebuilt.
    if flavour == Flavour::Desktop {
        crate::desktop_apps::check(root)?;
    }
    Ok(())
}

/// The overlay is the only place of what it writes: refuses a non-empty
/// regular file of `overlay` that `rootfs`, the system below it, also has
/// with the same content (a rootfs that already carries the live session).
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
                "the live layer's {} is also in the rootfs {}; rebuild the rootfs",
                relative.display(),
                rootfs.display()
            );
        }
    }
    Ok(())
}
