//! Parts: the pieces a release is combined from, each built once for what it
//! is built from and kept (`docs/build-redesign.md`, section 0).
//!
//! A part lives at `.artifacts/parts/<distro>/<kind>/<key>/`
//! (`runs::parts_dir`). Its key is a SHA-256 over its kind, the distro and
//! its named inputs; `part.json` records the kind, the key, the inputs, its
//! trees, its output files (path, bytes, sha256) and when it was finished.
//!
//! - `system`: the base system tree (the rootfs
//!   recipe's tree with the rootfs tools, the release's group recipes, the
//!   kernel's modules, finalized metadata, then Recipe's `base` entry from
//!   `record-base`) in `rootfs-source/`, its rootfs EROFS, the
//!   installed initramfs and the release set (`release-set.json`, `live/` and
//!   `boot/` members, `system-sources.sha256`). Built from the checkout's
//!   system sources, the builder's own crates, Recipe's and Ginux's declared
//!   behaviour versions (and their bytes in exact mode) and the Ginux root,
//!   the kernel and busybox (`system_inputs`).
//! - `overlay-<product>`: the product's overlay tree (live session, overlay
//!   tools, install screen; on the release medium the default software set
//!   and its package cache) built on the system tree, and its
//!   EROFS. Built from the system part (its key and its exact build), the
//!   product, the overlay tools' crates and the running builder itself
//!   (`overlay_inputs`).
//!
//! A part is built in a sibling temporary directory and renamed into place
//! only when complete, so nothing incomplete is ever under a key. Before
//! publishing, its inputs are computed again; if they changed while it was
//! built (the checkout was edited, a kernel installed), the result is thrown
//! away and the part is built again for the new inputs. A part found under
//! its key is used only when its record names exactly the inputs computed
//! now and every output file still has its recorded size and hash; anything
//! else is removed and built again. An `flock` on `<kind>/.<key>.lock`
//! serialises builders of one key: a part is checked and used under a shared
//! lock, built or removed only under an exclusive one, so two products or
//! two distros can build at once and a second builder of the same key
//! reuses the first one's result. The caller holds the shared lock for as
//! long as it holds the `Part`.

use anyhow::{bail, ensure, Context, Result};
use distro_contract::{runs, ProductId, Variant};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};

use crate::kernel::{KERNEL_IMAGE_PATH, KERNEL_MODULES_DIR};
use crate::pipeline::io::create_empty_overlay_dir;
use crate::pipeline::live_tools::{ToolLayer, LIVE_TOOLS};
use crate::pipeline::products::{self, ROOTFS_SOURCE_DIR};
use crate::pipeline::sources;

/// What a part holds and how it is laid out. Bump it whenever that changes in
/// a way no other input shows: it is in every key.
const FORMAT: &str = "1";
/// The part's record, in its directory.
const RECORD: &str = "part.json";
/// How often a part is built again when its inputs changed while it was built.
const ATTEMPTS: usize = 3;
/// Where the builder finds the compiler image when `GINUX_ROOTFS` is unset
/// (`recipe::compiler_image`).
const DEFAULT_GINUX_ROOTFS: &str = ".artifacts/ginux-env/prepared-image-build-tools/rootfs.tar";

/// A part's named inputs, each a digest or an exact value.
pub(crate) type Inputs = BTreeMap<String, String>;

/// One output file of a part, relative to its directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Output {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

/// `part.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema_version: u32,
    pub format: String,
    pub kind: String,
    pub distro_id: String,
    pub key: String,
    pub inputs: Inputs,
    /// The part's directory trees (only read once the part exists).
    pub trees: Vec<String>,
    /// Every file outside the trees.
    pub outputs: Vec<Output>,
    pub finished_at_utc: String,
}

/// A part ready to use; the shared lock on its key is held until it drops.
pub(crate) struct Part {
    pub kind: String,
    pub key: String,
    pub dir: PathBuf,
    pub record: Record,
    _lock: fs::File,
}

impl Part {
    /// The part's one tree.
    pub fn tree(&self) -> PathBuf {
        self.dir.join(&self.record.trees[0])
    }

    /// The key's first 16 hex digits, as the build log shows it.
    pub fn short_key(&self) -> &str {
        &self.key[..16]
    }

    /// Links every output file into `dir` at its relative path (a copy where a
    /// hard link cannot cross filesystems). Never moves anything out of the
    /// part, and refuses to replace a file already in `dir`.
    pub fn link_outputs(&self, dir: &Path) -> Result<()> {
        for output in &self.record.outputs {
            let relative = checked_relative(&output.path)?;
            let source = self.dir.join(relative);
            let destination = dir.join(relative);
            if fs::symlink_metadata(&destination).is_ok() {
                bail!(
                    "{} already exists; the {} part's {} would replace it",
                    destination.display(),
                    self.kind,
                    output.path
                );
            }
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
            }
            match fs::hard_link(&source, &destination) {
                Ok(()) => {}
                Err(error) if error.raw_os_error() == Some(libc::EXDEV) => {
                    fs::copy(&source, &destination).with_context(|| {
                        format!("copying {} to {}", source.display(), destination.display())
                    })?;
                }
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("linking {} to {}", source.display(), destination.display())
                    })
                }
            }
        }
        Ok(())
    }

    /// Makes `link` a symbolic link to the part's tree, for readers of a
    /// release directory (`PreparedProduct::read`); nothing writes through it.
    pub fn link_tree(&self, link: &Path) -> Result<()> {
        let tree = fs::canonicalize(self.tree())
            .with_context(|| format!("resolving {}", self.tree().display()))?;
        std::os::unix::fs::symlink(&tree, link)
            .with_context(|| format!("linking {} to {}", link.display(), tree.display()))
    }
}

/// The system part of `variant`, with the kernel in `kernel`
/// (`runs::kernel_dir`): built if no usable part has its key.
pub(crate) fn system(variant: &Variant, kernel: &Path) -> Result<Part> {
    let kind = "system".to_string();
    let names = &variant.contract.artifacts;
    let label = label(&kind, &variant.id);
    ensure_part(
        variant,
        &kind,
        &[ROOTFS_SOURCE_DIR],
        &|| system_inputs(variant, kernel),
        &mut |dir, inputs| {
            let seq = crate::timeline::Seq::new();
            let step = |what: &str| {
                crate::tprintln!("{label} {what}");
                seq.step(what);
                format!("{what} for the {kind} part of {}", variant.id)
            };
            let release = &inputs["kernel-release"];
            let modules = kernel.join(KERNEL_MODULES_DIR).join(release);
            let rootfs = create_empty_overlay_dir(dir, ROOTFS_SOURCE_DIR)?;

            let what = step("preparing the system tree");
            products::prepare_system(variant, &rootfs)
                .with_context(|| format!("preparing the {} system rootfs", variant.id))
                .context(what)?;

            let what = step("adding the kernel modules to the rootfs");
            crate::release::add_modules(&rootfs, &modules, release).context(what)?;

            let what = step("writing the rootfs image");
            levitate_recipe::target::finalize(&rootfs)
                .context("finalizing intended release metadata")
                .context(what.clone())?;
            // Recipe's `record-base` is the tree's very last change: the
            // receipt entry `base` and usr/lib/levitate/release.json.
            crate::update_release::record_base(variant, &rootfs).context(what.clone())?;
            crate::build_erofs_default(&rootfs, &dir.join(&names.rootfs)).context(what)?;

            let what = step("acquiring busybox");
            let busybox = crate::release::busybox(variant).context(what)?;

            // Every product carries the installed system, on every variant, so
            // every ISO carries the initramfs it boots through (recsetup
            // installs it). It is built from the system tree the ISO installs,
            // so its fsck, blkid and modules match the installed system.
            let what = step("building the installed initramfs");
            let initramfs_installed = dir.join(recinit::live::INITRAMFS_INSTALLED_FILENAME);
            recinit::build_install_initramfs(
                &recinit::InstallConfig {
                    rootfs: rootfs.clone(),
                    busybox_path: busybox,
                    modules_dir: modules.clone(),
                    output: initramfs_installed.clone(),
                    gzip_level: recinit::DEFAULT_GZIP_LEVEL,
                },
                false,
            )
            .context(what)?;

            let what = step("publishing the installed-system release manifest");
            sources::record(dir, &inputs["system-sources"])?;
            crate::release_manifest::write(
                variant,
                &rootfs,
                dir,
                release,
                &kernel.join(KERNEL_IMAGE_PATH),
                &initramfs_installed,
            )
            .context(what)?;
            seq.finish();
            Ok(())
        },
    )
}

/// `product`'s overlay part, built on `system` (the system part).
pub(crate) fn overlay(variant: &Variant, product: ProductId, system: &Part) -> Result<Part> {
    let kind = format!("overlay-{product}");
    let build = crate::product::build(product);
    let label = label(&kind, &variant.id);
    ensure_part(
        variant,
        &kind,
        &[build.overlay_dir],
        &|| overlay_inputs(variant, product, system),
        &mut |dir, _| {
            let seq = crate::timeline::Seq::new();
            crate::tprintln!("{label} preparing the overlay");
            seq.step("preparing the overlay");
            let overlay = create_empty_overlay_dir(dir, build.overlay_dir)?;
            products::build_overlay(variant, product, &system.tree(), &overlay)
                .with_context(|| format!("preparing the {product} overlay of {}", variant.id))?;
            crate::tprintln!("{label} writing the overlay image");
            seq.step("writing the overlay image");
            crate::build_overlayfs_default(&overlay, &dir.join(&variant.contract.artifacts.overlay))
                .with_context(|| format!("writing the {product} overlay image of {}", variant.id))?;
            seq.finish();
            Ok(())
        },
    )
}

/// Fails unless the kernel in `kernel` is still the one `system` was built
/// for (its release, image and modules), so what the combiner adds from the
/// kernel (the live initramfs's modules) matches the part.
pub(crate) fn check_kernel(system: &Part, kernel: &Path) -> Result<()> {
    let release = crate::release::kernel_release(kernel)?;
    let found = [
        ("kernel-release", release.clone()),
        ("kernel-image", file_sha256(&kernel.join(KERNEL_IMAGE_PATH))?.0),
        ("kernel-modules", tree_digest(&kernel.join(KERNEL_MODULES_DIR).join(&release))?),
    ];
    for (name, value) in found {
        ensure!(
            system.record.inputs.get(name) == Some(&value),
            "the kernel in {} changed since the {} part {} was built for it ({name}); build again",
            kernel.display(),
            system.kind,
            system.short_key()
        );
    }
    Ok(())
}

/// What the system part is built from. Each input is named in `part.json`;
/// none carries secret bytes.
fn system_inputs(variant: &Variant, kernel: &Path) -> Result<Inputs> {
    let repo = &variant.repo_root;
    let mut inputs = Inputs::new();
    // The checkout's builder, contract, profile, variant, apps, recipes, rootfs
    // tools and the Ginux launcher.
    inputs.insert(
        "system-sources".into(),
        sources::system_sources_digest(repo, &variant.id).context("fingerprinting the system's sources")?,
    );
    // The crates the builder itself is compiled from (recinit, reciso,
    // levitate-recipe, the contract...): they write the image, the installed
    // initramfs and the release set.
    let builder_crates = crate::recipe::tools::crate_closure_dirs(repo, Path::new("distro/builder"))?;
    inputs.insert(
        "builder-sources".into(),
        sources::roots_digest(repo, &builder_crates).context("fingerprinting the builder's crates")?,
    );
    // The Recipe and Ginux every recipe step runs under: by their declared
    // behaviour versions, as Recipe names them in its saved-state identities;
    // in exact mode also by the bytes of the Recipe CLI (the worker too,
    // unless a root needs the static one, which is built from the same
    // sources) and of the launcher, so a release comes only from these exact
    // tools. Recipe's sources are in "system-sources" and "builder-sources"
    // either way: the system ships Recipe.
    inputs.insert("recipe-behaviour".into(), levitate_recipe::identity::declared());
    if crate::recipe::exact_identity() {
        inputs.insert("recipe-identity".into(), "exact".into());
        inputs.insert("recipe-cli".into(), file_sha256(crate::recipe::binary(repo)?)?.0);
        inputs.insert(
            "ginux-launcher".into(),
            file_sha256(crate::recipe::ginux_binary(repo)?)?.0,
        );
    } else {
        inputs.insert("recipe-identity".into(), "declared".into());
    }
    // The Ginux root recipe steps run in.
    inputs.insert(
        "ginux-root".into(),
        std::env::var("GINUX_ROOT").unwrap_or_else(|_| "pinned compiler image".into()),
    );
    let rootfs_tar = std::env::var_os("GINUX_ROOTFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join(DEFAULT_GINUX_ROOTFS));
    inputs.insert(
        "ginux-rootfs".into(),
        if rootfs_tar.is_file() {
            file_sha256(&rootfs_tar)?.0
        } else {
            format!("absent: {}", rootfs_tar.display())
        },
    );
    // The kernel the system's modules come from.
    let release = crate::release::kernel_release(kernel)?;
    inputs.insert("kernel-image".into(), file_sha256(&kernel.join(KERNEL_IMAGE_PATH))?.0);
    inputs.insert(
        "kernel-modules".into(),
        tree_digest(&kernel.join(KERNEL_MODULES_DIR).join(&release))?,
    );
    inputs.insert("kernel-release".into(), release);
    inputs.insert(
        "busybox".into(),
        file_sha256(&crate::release::busybox(variant).context("acquiring busybox")?)?.0,
    );
    Ok(inputs)
}

/// What `product`'s overlay part is built from: its system part (which
/// covers the checkout's builder, recipes, TUI apps and contract), the
/// product, the overlay tools' crates, which the system's sources do not
/// cover, and the running builder executable, which compiles in the live
/// session's and the install screen's contents; overlays are cheap to build
/// again, so any builder change does.
fn overlay_inputs(variant: &Variant, product: ProductId, system: &Part) -> Result<Inputs> {
    let repo = &variant.repo_root;
    let mut inputs = Inputs::new();
    inputs.insert("system-part".into(), system.key.clone());
    // The exact build of that system, not only its key: the overlay copies
    // some of its files (the accounts), so a system part built again under
    // the same key builds the overlay again too.
    inputs.insert(
        "system-part-outputs".into(),
        hex(&Sha256::digest(serde_json::to_vec(&(&system.record.outputs, &system.record.finished_at_utc))?)),
    );
    inputs.insert("product".into(), product.name().into());
    let mut crates = BTreeSet::new();
    for tool in LIVE_TOOLS.iter().filter(|tool| tool.layer == ToolLayer::Overlay) {
        crates.extend(crate::recipe::tools::tool_crate_dirs(repo, tool.package)?);
    }
    inputs.insert(
        "overlay-tool-sources".into(),
        sources::roots_digest(repo, &crates).context("fingerprinting the overlay tools' crates")?,
    );
    // The executable this process runs, even if its file was replaced since.
    inputs.insert("builder-executable".into(), file_sha256(Path::new("/proc/self/exe"))?.0);
    Ok(inputs)
}

/// The part of `kind` for the inputs `inputs` computes: the one under its key
/// if usable, else built by `build` into an empty directory.
fn ensure_part(
    variant: &Variant,
    kind: &str,
    trees: &[&str],
    inputs: &dyn Fn() -> Result<Inputs>,
    build: &mut dyn FnMut(&Path, &Inputs) -> Result<()>,
) -> Result<Part> {
    let parent = runs::parts_dir(&variant.repo_root, &variant.id).join(kind);
    fs::create_dir_all(&parent).with_context(|| format!("creating {}", parent.display()))?;
    let label = label(kind, &variant.id);
    let ensure = crate::timeline::Step::begin("part", &format!("part {kind}"));
    let trees: Vec<String> = trees.iter().map(|tree| tree.to_string()).collect();
    for _ in 0..ATTEMPTS {
        let wanted = inputs().with_context(|| format!("computing the inputs of the {kind} part"))?;
        let key = key(kind, &variant.id, &wanted);
        let short = &key[..16];
        ensure.key(short);
        let dir = parent.join(&key);
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(parent.join(format!(".{key}.lock")))
            .with_context(|| format!("opening the lock of the {kind} part {short}"))?;
        let part = |record: Record, lock: fs::File| Part {
            kind: kind.to_string(),
            key: key.clone(),
            dir: dir.clone(),
            record,
            _lock: lock,
        };

        flock(&lock, libc::LOCK_SH, kind)?;
        if let Ok(Some(record)) = check(&dir, kind, &variant.id, &key, &wanted, &trees) {
            crate::tprintln!("{label} reused {short}");
            ensure.note("reused");
            ensure.finish();
            return Ok(part(record, lock));
        }
        // Not atomic: check again once no one else uses or builds the part.
        flock(&lock, libc::LOCK_EX, kind)?;
        match check(&dir, kind, &variant.id, &key, &wanted, &trees) {
            Ok(Some(record)) => {
                flock(&lock, libc::LOCK_SH, kind)?;
                crate::tprintln!("{label} reused {short}");
                ensure.note("reused");
                ensure.finish();
                return Ok(part(record, lock));
            }
            Ok(None) => {}
            Err(why) => {
                crate::tprintln!("{label} the part under {short} is unusable ({why:#}); removing it");
                fs::remove_dir_all(&dir).with_context(|| format!("removing {}", dir.display()))?;
            }
        }

        crate::tprintln!("{label} building {short}");
        remove_stale_builds(&parent, &key)?;
        let pending = parent.join(format!(".tmp-{key}-{}", std::process::id()));
        fs::create_dir(&pending).with_context(|| format!("creating {}", pending.display()))?;
        let built = build(&pending, &wanted)
            .and_then(|()| inputs().context("computing the part's inputs again after building it"));
        let found = match built {
            Ok(found) => found,
            Err(error) => {
                let _ = fs::remove_dir_all(&pending);
                return Err(error.context(format!("building the {kind} part {short} of {}", variant.id)));
            }
        };
        if found != wanted {
            let changed: Vec<&str> = wanted
                .keys()
                .chain(found.keys())
                .filter(|name| wanted.get(*name) != found.get(*name))
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            crate::tprintln!(
                "{label} its inputs changed while it was built ({}); discarding it and building again",
                changed.join(", ")
            );
            fs::remove_dir_all(&pending).with_context(|| format!("removing {}", pending.display()))?;
            continue;
        }
        let record = finish(&pending, kind, &variant.id, &key, wanted, &trees)
            .inspect_err(|_| {
                let _ = fs::remove_dir_all(&pending);
            })
            .with_context(|| format!("recording the {kind} part {short}"))?;
        fs::rename(&pending, &dir)
            .with_context(|| format!("moving {} into place at {}", pending.display(), dir.display()))?;
        fs::File::open(&parent)
            .and_then(|parent| parent.sync_all())
            .with_context(|| format!("syncing {}", parent.display()))?;
        flock(&lock, libc::LOCK_SH, kind)?;
        crate::tprintln!("{label} built {short}");
        ensure.note("built");
        ensure.finish();
        return Ok(part(record, lock));
    }
    bail!(
        "the inputs of the {kind} part of {} kept changing while it was built ({ATTEMPTS} attempts); \
         build again once the checkout and the kernel stay unchanged",
        variant.id
    )
}

/// Removes the parts of `distro_id` that no run directory under its output
/// directory (`runs::distro_output_dir`: every release product's runs and
/// every scenario's) links to any more, except the parts in `keep` (the ones
/// the calling build just used), and the unfinished builds (`.tmp-<key>-<pid>`)
/// of builders that died. Every key is handled under its exclusive lock,
/// taken without waiting: a part in use or being built (its lock held) is
/// left alone, and while the lock is held no builder can link the part. Only
/// then are the run directories read for what links a part: a symbolic link
/// into its directory (`Part::link_tree`) or a hard link to one of its files
/// (`Part::link_outputs`). Removal never follows a link and never leaves
/// `.artifacts/parts/<distro>/<kind>/`. The lock files stay: a builder may
/// hold one open, and a new file under the same name would not exclude it.
/// One line per removed part, with the bytes its files held (files a run or
/// the download store still links stay on disk). An error with one part is
/// a warning; the others are still handled.
pub fn prune(repo_root: &Path, distro_id: &str, keep: &[PathBuf]) -> Result<()> {
    let parts = runs::parts_dir(repo_root, distro_id);
    let root = match fs::canonicalize(&parts) {
        Ok(root) => root,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("resolving {}", parts.display())),
    };
    let keep: BTreeSet<PathBuf> = keep.iter().filter_map(|dir| fs::canonicalize(dir).ok()).collect();
    let label = format!("[parts:{distro_id}]");

    // Every key of every kind with its part and unfinished builds, locked.
    struct Locked {
        kind_dir: PathBuf,
        kind: String,
        key: String,
        part: Option<PathBuf>,
        unfinished: Vec<PathBuf>,
        _lock: fs::File,
    }
    let mut locked = Vec::new();
    let mut in_use = 0usize;
    for kind in fs::read_dir(&root).with_context(|| format!("listing {}", root.display()))? {
        let kind = kind?;
        let Some(kind_name) = kind.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if kind_name.starts_with('.') || !kind.file_type()?.is_dir() {
            continue;
        }
        let kind_dir = fs::canonicalize(kind.path())?;
        ensure!(kind_dir.parent() == Some(root.as_path()), "{} is not inside {}", kind_dir.display(), root.display());
        let mut keys: BTreeMap<String, (Option<PathBuf>, Vec<PathBuf>)> = BTreeMap::new();
        for entry in fs::read_dir(&kind_dir).with_context(|| format!("listing {}", kind_dir.display()))? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if is_key(&name) {
                keys.entry(name).or_default().0 = Some(entry.path());
            } else if let Some(key) = name
                .strip_prefix(".tmp-")
                .and_then(|rest| rest.rsplit_once('-'))
                .map(|(key, _pid)| key)
                .filter(|key| is_key(key))
            {
                keys.entry(key.to_string()).or_default().1.push(entry.path());
            }
        }
        for (key, (part, unfinished)) in keys {
            if part.as_ref().is_some_and(|dir| keep.contains(dir)) {
                continue;
            }
            let lock = fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(kind_dir.join(format!(".{key}.lock")))
                .with_context(|| format!("opening the lock of the {kind_name} part {}", &key[..16]))?;
            if !try_lock_exclusive(&lock)? {
                in_use += 1;
                continue;
            }
            locked.push(Locked {
                kind_dir: kind_dir.clone(),
                kind: kind_name.clone(),
                key,
                part,
                unfinished,
                _lock: lock,
            });
        }
    }
    if locked.is_empty() {
        return Ok(());
    }

    // What the runs link, read while no builder can link a locked part.
    let referenced = run_references(&runs::distro_output_dir(repo_root, distro_id), &[&parts, &root])?;

    let (mut removed, mut freed, mut kept) = (0usize, 0u64, keep.len() + in_use);
    for item in &locked {
        let short = &item.key[..16];
        for dir in &item.unfinished {
            // Under the key's exclusive lock no build of it runs (`ensure_part`).
            match remove_inside(dir, &item.kind_dir) {
                Ok(()) => crate::tprintln!("{label} removed the unfinished build {}", dir.display()),
                Err(error) => crate::tprintln!("{label} warning: could not remove {}: {error:#}", dir.display()),
            }
        }
        let Some(dir) = &item.part else {
            continue;
        };
        if referenced.parts.contains(&(item.kind.clone(), item.key.clone())) {
            kept += 1;
            continue;
        }
        let outcome = (|| -> Result<Option<u64>> {
            let mut seen = HashSet::new();
            let mut bytes = 0u64;
            for entry in walkdir::WalkDir::new(dir).follow_links(false) {
                let entry = entry.with_context(|| format!("walking {}", dir.display()))?;
                if !entry.file_type().is_file() {
                    continue;
                }
                let meta = entry.metadata()?;
                let inode = (meta.dev(), meta.ino());
                if referenced.inodes.contains(&inode) {
                    return Ok(None);
                }
                if seen.insert(inode) {
                    bytes += meta.blocks() * 512;
                }
            }
            remove_inside(dir, &item.kind_dir)?;
            Ok(Some(bytes))
        })();
        match outcome {
            Ok(Some(bytes)) => {
                removed += 1;
                freed += bytes;
                crate::tprintln!("{label} removed the {} part {short} ({})", item.kind, gib(bytes));
            }
            Ok(None) => kept += 1,
            Err(error) => {
                kept += 1;
                crate::tprintln!("{label} warning: could not remove the {} part {short}: {error:#}", item.kind);
            }
        }
    }
    if removed > 0 {
        crate::tprintln!("{label} removed {removed} part(s) no run links ({}); kept {kept}", gib(freed));
    }
    Ok(())
}

/// What the run directories link into the parts.
struct References {
    /// `(kind, key)` of each part a symbolic link points into.
    parts: BTreeSet<(String, String)>,
    /// Every regular file with more than one link.
    inodes: HashSet<(u64, u64)>,
}

/// Reads every run directory under `out` without following links: the parts
/// (under any of `parts`, spellings of one directory) its symbolic links point
/// into, and the files it shares by hard link. An entry that vanished while
/// it was read (a run pruned meanwhile) links nothing; any other error fails.
fn run_references(out: &Path, parts: &[&Path]) -> Result<References> {
    let mut found = References {
        parts: BTreeSet::new(),
        inodes: HashSet::new(),
    };
    if fs::symlink_metadata(out).is_err() {
        return Ok(found);
    }
    for entry in walkdir::WalkDir::new(out).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if error.io_error().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) => continue,
            Err(error) => return Err(error).with_context(|| format!("reading {}", out.display())),
        };
        let file_type = entry.file_type();
        if file_type.is_symlink() {
            let Ok(target) = fs::read_link(entry.path()) else {
                continue;
            };
            let target = match entry.path().parent() {
                Some(parent) if target.is_relative() => parent.join(target),
                _ => target,
            };
            let target = lexical(&target);
            for prefix in parts {
                if let Ok(rest) = target.strip_prefix(prefix) {
                    let mut names = rest.components().filter_map(|part| match part {
                        Component::Normal(name) => name.to_str().map(str::to_string),
                        _ => None,
                    });
                    if let (Some(kind), Some(key)) = (names.next(), names.next()) {
                        found.parts.insert((kind, key));
                    }
                }
            }
        } else if file_type.is_file() {
            match entry.metadata() {
                Ok(meta) if meta.nlink() > 1 => {
                    found.inodes.insert((meta.dev(), meta.ino()));
                }
                Ok(_) => {}
                Err(error) if error.io_error().is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) => {}
                Err(error) => return Err(error).with_context(|| format!("reading {}", entry.path().display())),
            }
        }
    }
    Ok(found)
}

/// `path` with `.` and `..` resolved by name, never by reading the disk.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// Whether `name` is a part key: 64 lowercase hex digits.
fn is_key(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

/// Removes the directory `dir`, which must be a real directory (not a link)
/// directly in `kind_dir` (a canonical kind directory), without following any
/// link inside it.
fn remove_inside(dir: &Path, kind_dir: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(dir).with_context(|| format!("reading {}", dir.display()))?;
    ensure!(meta.file_type().is_dir(), "{} is not a directory", dir.display());
    let real = fs::canonicalize(dir).with_context(|| format!("resolving {}", dir.display()))?;
    ensure!(
        real.parent() == Some(kind_dir) && real.file_name() == dir.file_name(),
        "{} is not directly in {}",
        real.display(),
        kind_dir.display()
    );
    // remove_dir_all removes links inside the tree, never what they point to.
    fs::remove_dir_all(&real).with_context(|| format!("removing {}", real.display()))
}

/// Takes `file`'s exclusive lock if no one holds it; false if someone does.
fn try_lock_exclusive(file: &fs::File) -> Result<bool> {
    loop {
        // SAFETY: flock only reads the descriptor of the open lock file.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EWOULDBLOCK) => return Ok(false),
            Some(libc::EINTR) => continue,
            _ => return Err(error).context("locking a part"),
        }
    }
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1u64 << 30) as f64)
}

/// The prefix of the build log's lines about the `kind` part of `distro_id`:
/// `[part:<kind>:<distro>]`, with `:exact` in exact mode
/// (`crate::recipe::exact_identity`).
fn label(kind: &str, distro_id: &str) -> String {
    let mode = if crate::recipe::exact_identity() { ":exact" } else { "" };
    format!("[part:{kind}:{distro_id}{mode}]")
}

/// The key of the `kind` part of `distro_id` built from `inputs`.
fn key(kind: &str, distro_id: &str, inputs: &Inputs) -> String {
    let mut hasher = Sha256::new();
    let mut field = |name: &str, value: &str| {
        for text in [name, value] {
            hasher.update((text.len() as u64).to_le_bytes());
            hasher.update(text.as_bytes());
        }
    };
    field("format", FORMAT);
    field("kind", kind);
    field("distro", distro_id);
    for (name, value) in inputs {
        field(name, value);
    }
    hex(&hasher.finalize())
}

/// The record of the part in `dir` if it is the wanted part and intact;
/// `None` if there is none; an error naming what is wrong otherwise.
fn check(
    dir: &Path,
    kind: &str,
    distro_id: &str,
    key: &str,
    inputs: &Inputs,
    trees: &[String],
) -> Result<Option<Record>> {
    match fs::symlink_metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
        Ok(meta) if !meta.is_dir() => bail!("{} is not a directory", dir.display()),
        Ok(_) => {}
    }
    let path = dir.join(RECORD);
    let record: Record = serde_json::from_slice(
        &fs::read(&path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("parsing {}", path.display()))?;
    ensure!(
        record.schema_version == 1
            && record.format == FORMAT
            && record.kind == kind
            && record.distro_id == distro_id
            && record.key == key,
        "{} records another part",
        path.display()
    );
    ensure!(record.inputs == *inputs, "{} records other inputs", path.display());
    ensure!(record.trees == trees, "{} records other trees", path.display());
    for tree in trees {
        let tree = dir.join(checked_relative(tree)?);
        ensure!(
            fs::symlink_metadata(&tree).is_ok_and(|meta| meta.is_dir()),
            "{} is missing",
            tree.display()
        );
    }
    // Hard links to one file (live/filesystem.erofs) are hashed once.
    let mut hashed = HashMap::new();
    for output in &record.outputs {
        let file = dir.join(checked_relative(&output.path)?);
        let meta = fs::symlink_metadata(&file).with_context(|| format!("reading {}", file.display()))?;
        ensure!(
            meta.is_file() && meta.len() == output.bytes,
            "{} is not the {}-byte file it was",
            file.display(),
            output.bytes
        );
        let sha256 = match hashed.entry((meta.dev(), meta.ino())) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(file_sha256(&file)?.0),
        };
        ensure!(
            *sha256 == output.sha256,
            "{} changed since the part was built",
            file.display()
        );
    }
    Ok(Some(record))
}

/// Records the part built in `pending`: every file outside its trees, then
/// `part.json`, then flushes it all to disk.
fn finish(
    pending: &Path,
    kind: &str,
    distro_id: &str,
    key: &str,
    inputs: Inputs,
    trees: &[String],
) -> Result<Record> {
    for tree in trees {
        let tree = pending.join(tree);
        ensure!(
            fs::symlink_metadata(&tree).is_ok_and(|meta| meta.is_dir()),
            "the part has no tree {}",
            tree.display()
        );
    }
    let mut outputs = Vec::new();
    let mut hashed = HashMap::new();
    let walk = walkdir::WalkDir::new(pending)
        .follow_links(false)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() != 1 || !trees.iter().any(|tree| entry.file_name() == tree.as_str())
        });
    for entry in walk {
        let entry = entry?;
        if entry.file_type().is_dir() {
            continue;
        }
        let relative = entry.path().strip_prefix(pending)?;
        let name = relative
            .to_str()
            .with_context(|| format!("{} is not UTF-8", relative.display()))?;
        ensure!(
            entry.file_type().is_file(),
            "{} is neither a file nor a directory; a part's outputs are files",
            entry.path().display()
        );
        ensure!(name != RECORD, "the part wrote its own {RECORD}");
        let meta = entry.metadata()?;
        let sha256 = match hashed.entry((meta.dev(), meta.ino())) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(file_sha256(&pending.join(relative))?.0),
        }
        .clone();
        outputs.push(Output {
            path: name.to_string(),
            bytes: meta.len(),
            sha256,
        });
    }
    let record = Record {
        schema_version: 1,
        format: FORMAT.into(),
        kind: kind.into(),
        distro_id: distro_id.into(),
        key: key.into(),
        inputs,
        trees: trees.to_vec(),
        outputs,
        finished_at_utc: runs::now_rfc3339(),
    };
    runs::write_atomic(&pending.join(RECORD), &serde_json::to_vec_pretty(&record)?)?;
    let dir = fs::File::open(pending)?;
    // SAFETY: syncfs only reads the descriptor of the open directory.
    if unsafe { libc::syncfs(dir.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| format!("syncing {}", pending.display()));
    }
    Ok(record)
}

/// Removes what earlier builds of `key` left when they died; only called
/// under the key's exclusive lock, so no build of it is running.
fn remove_stale_builds(parent: &Path, key: &str) -> Result<()> {
    let prefix = format!(".tmp-{key}-");
    for entry in fs::read_dir(parent).with_context(|| format!("listing {}", parent.display()))? {
        let entry = entry?;
        if entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)) {
            crate::tprintln!("  removing the unfinished build {}", entry.path().display());
            fs::remove_dir_all(entry.path())
                .with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn flock(file: &fs::File, operation: libc::c_int, kind: &str) -> Result<()> {
    loop {
        // SAFETY: flock only reads the descriptor of the open lock file.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error).with_context(|| format!("locking the {kind} part"));
        }
    }
}

/// `path` if it is a plain relative path (no `..`, no root).
fn checked_relative(path: &str) -> Result<&Path> {
    let relative = Path::new(path);
    ensure!(
        !path.is_empty() && relative.components().all(|part| matches!(part, Component::Normal(_))),
        "{path:?} is not a plain path inside the part"
    );
    Ok(relative)
}

/// The SHA-256 of the file at `path` and its length.
pub(crate) fn file_sha256(path: &Path) -> Result<(String, u64)> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer).with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((hex(&hasher.finalize()), bytes))
}

/// A digest of the tree at `root`: every entry's relative path, type,
/// permission bits and content (a link's target).
fn tree_digest(root: &Path) -> Result<String> {
    ensure!(root.is_dir(), "{} is not a directory", root.display());
    let mut hasher = Sha256::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1).sort_by_file_name() {
        let entry = entry?;
        let relative = entry.path().strip_prefix(root)?;
        let name = relative.as_os_str().as_encoded_bytes();
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name);
        let meta = fs::symlink_metadata(entry.path())?;
        hasher.update((meta.permissions().mode() & 0o7777).to_le_bytes());
        if meta.file_type().is_symlink() {
            let target = fs::read_link(entry.path())?;
            hasher.update(b"l");
            hasher.update(target.as_os_str().as_encoded_bytes());
        } else if meta.is_dir() {
            hasher.update(b"d");
        } else if meta.is_file() {
            hasher.update(b"f");
            hasher.update(file_sha256(entry.path())?.0.as_bytes());
        } else {
            bail!("{} is not a file, directory or link", entry.path().display());
        }
        hasher.update([0]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
