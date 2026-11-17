//! What makes an image updatable, and the files a release build writes next
//! to its ISO (docs/release-engineering.md, sections 2, 3, 8 and 10).
//!
//! In the system part (every image):
//!
//! - [`add_release_recipes`]: the release's recipes ([`group_recipes`]: this
//!   OS's package groups of any kind, its other `system` and `library`
//!   recipes, and their dependencies) as base files at [`RELEASE_RECIPES`];
//!   they change only with a release, and the installed Recipe reads them
//!   there as the release's; each recipe's pins file (`NAME.pins`) and the
//!   modules it imports go beside it;
//! - [`record_base`]: Recipe's `record-base` on the finished tree, its very
//!   last change before the EROFS image: the receipt entry `base` and
//!   `usr/lib/levitate/release.json`. That file cannot name the digests of
//!   archives made from the tree it is in, so it is the release's identity
//!   alone (`schema`, `os`, `version`, `serial`, `date`, `notes`), which is all
//!   `record-base` and the installed system read.
//!
//! In a `release` product's run directory, beside the ISO ([`write`]):
//!
//! - `base.tar.zst`: the system tree as the image holds it, written by the
//!   same tar writer as the EROFS image (`levitate_recipe::target::write_tar`:
//!   the tree's target metadata, numeric owners, a member for every
//!   directory, extended attributes as pax `SCHILY.xattr.*` records, names in
//!   sorted order, every mtime 0), which is the format Recipe's
//!   `src/release.rs` reads;
//! - `boot.tar.zst`: exactly `vmlinuz` and `initramfs.img` (the installed
//!   initramfs), the two regular files `recab update` and `recupdate apply`
//!   take;
//! - `recipes.tar.zst`: the same group recipes and pins files as
//!   [`RELEASE_RECIPES`] (taken from the tree, so they are the same bytes),
//!   flat at its top level;
//! - `release.json`: the unsigned manifest (schema 1) with the archives'
//!   `url`, `sha256` and `size` left empty; `cargo xtask release publish`
//!   fills them, signs it and publishes.

use anyhow::{bail, ensure, Context, Result};
use distro_contract::Variant;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Where every image keeps its release's group recipes (relative to its root).
pub(crate) const RELEASE_RECIPES: &str = "usr/lib/recipe/release-recipes";
/// The unsigned update manifest beside the ISO.
pub(crate) const MANIFEST: &str = "release.json";
const BASE_ARCHIVE: &str = "base.tar.zst";
const BOOT_ARCHIVE: &str = "boot.tar.zst";
const RECIPES_ARCHIVE: &str = "recipes.tar.zst";
/// One mtime for every member of the small archives, as `write_tar` uses.
const MTIME: u64 = 0;

fn recipes_dir(variant: &Variant) -> PathBuf {
    variant.repo_root.join("recipes")
}

/// The names of the release's recipes for `variant`, sorted: of the
/// `recipes/*.rhai` for this OS (`ABOUT.systems` names the variant's
/// os-release `ID`, and `ABOUT.libc`, when given, its C library), every
/// package group (a `PACKAGES` map with this C library's key: pinned by
/// `cargo xtask base groups` against the same package snapshot as the base,
/// whatever its `kind`) and every recipe of `kind` `system` or `library`
/// without `PACKAGES`; then the runtime dependency closure (`deps`) of all of
/// them. All of it is read by the engine (`needs::declaration`).
pub(crate) fn group_recipes(variant: &Variant) -> Result<Vec<String>> {
    use levitate_recipe::declarations::Packages;
    let dir = recipes_dir(variant);
    let os = &variant.contract.identity.os_id;
    let libc = match variant.contract.profile.libc {
        distro_contract::profile::Libc::Glibc => "glibc",
        distro_contract::profile::Libc::Musl => "musl",
    };
    let mut pending = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "rhai") {
            continue;
        }
        // A module (`os-facts.rhai`, `look.rhai`) is imported by recipes and
        // is no recipe: the engine's own rule (the one `recipe index` uses).
        if !path.is_file() || levitate_recipe::catalog::is_module(&path) {
            continue;
        }
        let declaration = levitate_recipe::needs::declaration(&path, Some(&dir))
            .with_context(|| format!("reading {}", path.display()))?;
        let declared = &declaration.declared;
        let about = declared.about.clone().unwrap_or_default();
        let listed = |key: &str, value: &str| about[key].as_array().is_some_and(|l| l.iter().any(|s| s.as_str() == Some(value)));
        if !listed("systems", os) || (about["libc"].is_array() && !listed("libc", libc)) {
            continue;
        }
        let wanted = match &declared.packages {
            Some(Packages::ByLibc { keys, .. }) => keys.contains(libc),
            Some(Packages::Any(_)) => false,
            None => matches!(about["kind"].as_str(), Some("system" | "library")),
        };
        if wanted {
            pending.push(path);
        }
    }
    ensure!(!pending.is_empty(), "no recipe in {} is a system group of {os}", dir.display());
    // The runtime dependency closure, by the engine's reading of `deps`.
    let mut names = std::collections::BTreeSet::new();
    while let Some(path) = pending.pop() {
        let name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .with_context(|| format!("{} has no UTF-8 name", path.display()))?
            .to_owned();
        if !names.insert(name) {
            continue;
        }
        let declaration = levitate_recipe::needs::declaration(&path, Some(&dir))
            .with_context(|| format!("reading {}", path.display()))?;
        pending.extend(declaration.dependencies.iter().filter(|d| !d.build_only).map(|d| d.recipe.clone()));
    }
    Ok(names.into_iter().collect())
}

/// Every file of `recipes/` that the recipe `name` is compiled from, by the
/// engine's own reading: the recipe, the base it extends, its pins file and
/// the modules it imports (`os-facts.rhai`, `look.rhai`), canonical paths.
pub(crate) fn recipe_files(variant: &Variant, name: &str) -> Result<Vec<PathBuf>> {
    let dir = recipes_dir(variant);
    let recipe = dir.join(format!("{name}.rhai"));
    let declaration = levitate_recipe::needs::declaration(&recipe, Some(&dir))
        .with_context(|| format!("reading recipes/{name}.rhai"))?;
    let mut files: Vec<PathBuf> = declaration.source_digests.keys().cloned().collect();
    files.extend(declaration.declared.modules.iter().cloned());
    let folder = dir.canonicalize().with_context(|| format!("resolving {}", dir.display()))?;
    for file in &files {
        ensure!(
            file.parent() == Some(folder.as_path()),
            "recipes/{name}.rhai is compiled from {}, which is not in {}",
            file.display(),
            dir.display()
        );
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Puts the release's group recipes into the system tree `rootfs` at
/// [`RELEASE_RECIPES`] (directory 0755, files 0644, owned by root), before
/// `record-base`, so they are files of the entry `base`.
pub(crate) fn add_release_recipes(variant: &Variant, rootfs: &Path) -> Result<()> {
    let names = group_recipes(variant)?;
    let dest = rootfs.join(RELEASE_RECIPES);
    if fs::symlink_metadata(&dest).is_ok() {
        fs::remove_dir_all(&dest).with_context(|| format!("removing {}", dest.display()))?;
    }
    fs::create_dir_all(&dest).with_context(|| format!("creating {}", dest.display()))?;
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))?;
    let mut copied = std::collections::BTreeSet::new();
    for name in &names {
        for source in recipe_files(variant, name)? {
            let file_name = source
                .file_name()
                .and_then(|n| n.to_str())
                .with_context(|| format!("{} has no file name", source.display()))?
                .to_owned();
            if !copied.insert(file_name.clone()) {
                continue;
            }
            let file = dest.join(&file_name);
            fs::copy(&source, &file)
                .with_context(|| format!("copying recipes/{file_name} into {}", dest.display()))?;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o644))?;
            chown_root(&file)?;
        }
    }
    chown_root(&dest)?;
    crate::tprintln!(
        "[release] {} group recipes of {} in /{RELEASE_RECIPES}",
        names.len(),
        variant.contract.identity.os_id
    );
    Ok(())
}

fn chown_root(path: &Path) -> Result<()> {
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        std::os::unix::fs::lchown(path, Some(0), Some(0))
            .with_context(|| format!("giving {} to root", path.display()))?;
    }
    Ok(())
}

/// The release's identity as the image records it (and `record-base` reads).
fn identity(variant: &Variant) -> serde_json::Value {
    let identity = &variant.contract.identity;
    serde_json::json!({
        "schema": 1,
        "os": identity.os_id,
        "version": identity.os_version,
        "serial": identity.release_serial,
        "date": today(),
        "notes": identity.release_notes,
    })
}

fn today() -> String {
    distro_contract::runs::now_rfc3339()[..10].to_string()
}

/// Recipe's `record-base` on the finished tree `rootfs` (the same function
/// `recipe --system --root TREE record-base --release FILE` runs): the entry
/// `base` in the tree's receipt and its `usr/lib/levitate/release.json`. The
/// very last change to the tree; the EROFS image is made right after.
pub(crate) fn record_base(variant: &Variant, rootfs: &Path) -> Result<()> {
    let manifest = tempfile::Builder::new().prefix("release-identity-").suffix(".json").tempfile()?;
    let mut text = serde_json::to_string_pretty(&identity(variant))?;
    text.push('\n');
    fs::write(manifest.path(), text)?;
    let recorded = levitate_recipe::release::record_base(rootfs, manifest.path())
        .with_context(|| format!("recording the base of {}", rootfs.display()))?;
    crate::tprintln!(
        "[release] recorded the base: {} paths, release {} (serial {})",
        recorded.files,
        recorded.version,
        recorded.serial
    );
    Ok(())
}

/// Writes the update release next to the ISO in `dir`: the three archives
/// and the unsigned `release.json`. `rootfs` is the system part's tree (only
/// read), `kernel` and `initramfs` the installed system's kernel and initramfs
/// the ISO carries.
pub(crate) fn write(variant: &Variant, rootfs: &Path, kernel: &Path, initramfs: &Path, dir: &Path) -> Result<()> {
    let id = &variant.contract.identity;
    let installed: serde_json::Value = serde_json::from_slice(
        &fs::read(rootfs.join(levitate_recipe::release::INSTALLED))
            .context("the system tree records no release; its part predates record-base")?,
    )?;
    ensure!(
        installed["serial"].as_u64() == Some(id.release_serial)
            && installed["version"].as_str() == Some(id.os_version.as_str())
            && installed["notes"].as_str() == Some(id.release_notes.as_str()),
        "the system tree records release {} (serial {}), the variant names {} (serial {})",
        installed["version"],
        installed["serial"],
        id.os_version,
        id.release_serial
    );

    crate::tprintln!("[release] writing {BASE_ARCHIVE} (the base tree, zstd -19)");
    compress_into(&dir.join(BASE_ARCHIVE), |out| {
        levitate_recipe::target::write_tar(rootfs, out).context("writing the base tree as tar")
    })?;

    crate::tprintln!("[release] writing {BOOT_ARCHIVE}");
    compress_into(&dir.join(BOOT_ARCHIVE), |out| {
        let mut tar = tar::Builder::new(out);
        for (name, file) in [("vmlinuz", kernel), ("initramfs.img", initramfs)] {
            append_file(&mut tar, name, file)?;
        }
        tar.finish()?;
        Ok(())
    })?;

    crate::tprintln!("[release] writing {RECIPES_ARCHIVE}");
    let recipes = rootfs.join(RELEASE_RECIPES);
    let mut names: Vec<String> = fs::read_dir(&recipes)
        .with_context(|| format!("reading {}", recipes.display()))?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect::<Result<_>>()?;
    names.sort();
    ensure!(!names.is_empty(), "{} holds no recipe", recipes.display());
    compress_into(&dir.join(RECIPES_ARCHIVE), |out| {
        let mut tar = tar::Builder::new(out);
        for name in &names {
            append_file(&mut tar, name, &recipes.join(name))?;
        }
        tar.finish()?;
        Ok(())
    })?;

    let empty = || serde_json::json!({ "url": "", "sha256": "", "size": 0 });
    // The image's own record, so the manifest names the same release (and
    // date) as the base it carries, also when the system part was reused.
    let mut manifest = installed.clone();
    let object = manifest.as_object_mut().context("the image's release record is an object")?;
    object.insert("base".into(), empty());
    object.insert("boot".into(), empty());
    object.insert("iso".into(), empty());
    object.insert(
        "recipes".into(),
        serde_json::json!({ "url": "", "sha256": "", "size": 0, "revision": recipes_revision(variant)? }),
    );
    let mut text = serde_json::to_string_pretty(&manifest)?;
    text.push('\n');
    distro_contract::runs::write_atomic(&dir.join(MANIFEST), text.as_bytes())?;
    crate::tprintln!(
        "[release] wrote the unsigned {MANIFEST} ({} {}, serial {}); publish with `cargo xtask release publish {}`",
        id.os_id,
        id.os_version,
        id.release_serial,
        variant.id
    );
    Ok(())
}

/// One regular member `name` (mode 0644, root, mtime 0) with `file`'s bytes.
fn append_file<W: Write>(tar: &mut tar::Builder<W>, name: &str, file: &Path) -> Result<()> {
    let mut input = levitate_recipe::fsx::open_regular(file)
        .with_context(|| format!("opening {}", file.display()))?;
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(MTIME);
    header.set_size(input.metadata()?.len());
    tar.append_data(&mut header, name, &mut input)
        .with_context(|| format!("archiving {}", file.display()))
}

/// Streams what `write` produces through `zstd -T0 -19` into `path` (written
/// as `<path>.part`, renamed when complete).
fn compress_into(path: &Path, write: impl FnOnce(&mut dyn Write) -> Result<()>) -> Result<()> {
    let partial = PathBuf::from(format!("{}.part", path.display()));
    let output = fs::File::create(&partial).with_context(|| format!("creating {}", partial.display()))?;
    let mut zstd = Command::new("zstd")
        .args(["-q", "-T0", "-19", "--no-progress", "-c"])
        .stdin(Stdio::piped())
        .stdout(output)
        .spawn()
        .context("starting zstd (the host needs it, as for the EROFS images)")?;
    let written = (|| -> Result<()> {
        let mut input = std::io::BufWriter::new(zstd.stdin.take().context("zstd has no stdin")?);
        write(&mut input)?;
        input.flush()?;
        Ok(())
    })();
    let status = zstd.wait().context("waiting for zstd")?;
    if let Err(error) = written {
        let _ = fs::remove_file(&partial);
        return Err(error.context(format!("writing {}", path.display())));
    }
    if !status.success() {
        let _ = fs::remove_file(&partial);
        bail!("zstd writing {} failed: {status}", path.display());
    }
    fs::rename(&partial, path).with_context(|| format!("moving {} into place", path.display()))
}

/// The `recipes` submodule's commit. Its working tree is what the release's
/// group recipes are taken from, so uncommitted changes there are named.
fn recipes_revision(variant: &Variant) -> Result<String> {
    let dir = recipes_dir(variant);
    let run = |args: &[&str]| -> Result<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(args)
            .output()
            .context("running git")?;
        ensure!(
            output.status.success(),
            "git {} in {} failed: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let revision = run(&["rev-parse", "HEAD"])?;
    let dirty = run(&["status", "--porcelain", "--untracked-files=no", "--", "."])?;
    if !dirty.is_empty() {
        crate::tprintln!(
            "[release] warning: recipes/ has uncommitted changes; the release's group recipes hold them, \
             so recipes.revision {revision} does not describe them exactly (commit recipes/ before a public release)"
        );
    }
    Ok(revision)
}
