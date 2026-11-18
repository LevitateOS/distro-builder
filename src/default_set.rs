//! The default software set on the live media (docs/product-handoff.md 2.4).
//!
//! No image carries software above the base: the variant's default set
//! (`[desktop].recipes`, group recipes of the public catalogue in `recipes/`)
//! is installed through Recipe into every target during the installation, by
//! recsetup's software step. This module puts on a live medium's overlay what
//! that needs, and nothing of it reaches an installed system (recstrap
//! extracts the system image, not the overlay):
//!
//! - [`write_medium`], on the release medium: the default set's list and its
//!   recipes with their deps (`install_tools::DEFAULT_SET_DIR`) go onto the
//!   overlay. The package cache (every file those recipes pin for the
//!   variant's C library, taken from the download store that the desktop
//!   lock's preseed fills; `os-recipes/release-medium.rhai` assembles it) does
//!   not: it is cut by category into one `parts-<category>.tar.zst` per
//!   `[[desktop.categories]]` of the variant (docs/release-engineering.md,
//!   section 11), written into the overlay part's directory, so they are
//!   recorded outputs of that part and the release run links them beside the
//!   ISO. A part holds the files with paths relative to
//!   `install_tools::RECIPE_CACHE_DIR`, so extracting it there installs it.
//! - [`add_live_network`], on every live medium: the `network` group and its
//!   deps, so the live session has NetworkManager. The builder produces that
//!   group for the variant's system (`RECIPE_HOST_LIBC_ROOT`, the base tree)
//!   and writes its system trees into the overlay with owners and accounts
//!   resolved by Recipe's system functions, as an install would; their own
//!   configuration enables NetworkManager.
//!
//! Rust reads the recipes' declarations (`let deps`, `SOURCES`) and their pins
//! files (`NAME.pins`, which travel with the recipes) and lays out
//! the inputs; the recipes own acquiring, checking and building.

use anyhow::{bail, ensure, Context, Result};
use distro_contract::profile::{Flavour, Libc};
use distro_contract::Variant;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::source::preseed_recipe;
use crate::recipe::{rootfs_source, Invocation};

/// The group the live session's network comes from.
const LIVE_NETWORK: &str = "network";
/// The OS recipe that assembles the release medium's default set tree.
const MEDIUM_RECIPE: &str = "distro/builder/os-recipes/release-medium.rhai";

fn recipes_dir(repo: &Path) -> PathBuf {
    repo.join("recipes")
}

fn recipe_file(repo: &Path, name: &str) -> PathBuf {
    recipes_dir(repo).join(format!("{name}.rhai"))
}

/// The pins file beside a group recipe (`NAME.pins`), when it has one.
fn pins_file(repo: &Path, name: &str) -> PathBuf {
    recipes_dir(repo).join(format!("{name}.pins"))
}

/// The names in a recipe's literal `let deps = [...]` line (none without one;
/// `build_deps` are only there while a recipe builds and are not read).
fn deps_of(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("let deps = ["))
        .map(|rest| {
            let list = rest.split(']').next().unwrap_or("");
            list.split('"').skip(1).step_by(2).map(str::to_string).collect()
        })
        .unwrap_or_default()
}

/// `names` with every recipe they depend on, each once, a recipe after its
/// deps (the order Recipe installs them in).
pub(crate) fn closure(repo: &Path, names: &[String]) -> Result<Vec<String>> {
    fn visit(repo: &Path, name: &str, path: &mut Vec<String>, done: &mut Vec<String>) -> Result<()> {
        if done.iter().any(|d| d == name) {
            return Ok(());
        }
        if path.iter().any(|p| p == name) {
            bail!("the group recipes depend on each other in a cycle: {} -> {name}", path.join(" -> "));
        }
        let file = recipe_file(repo, name);
        let text = fs::read_to_string(&file)
            .with_context(|| format!("reading the group recipe {} (recipes/{name}.rhai)", file.display()))?;
        path.push(name.to_string());
        for dep in deps_of(&text) {
            visit(repo, &dep, path, done)?;
        }
        path.pop();
        done.push(name.to_string());
        Ok(())
    }
    let mut done = Vec::new();
    for name in names {
        visit(repo, name, &mut Vec::new(), &mut done)?;
    }
    Ok(done)
}

fn is_digest(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The `[LIBC FORMAT]` sections of a pins file (specification 2.7) apply to
/// `libc` when `LIBC` is that C library or `any`.
fn pins_file_of(text: &str, libc: &str) -> Vec<(String, String)> {
    let mut pins = Vec::new();
    let mut applies = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("version ") {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let section_libc = section.split_whitespace().next().unwrap_or("");
            applies = section_libc == libc || section_libc == "any";
            continue;
        }
        if !applies {
            continue;
        }
        // NAME SHA256 BYTES VAR URL LICENCE
        let mut fields = line.split(' ');
        let (Some(_), Some(sha), Some(_), Some(_), Some(url)) =
            (fields.next(), fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if is_digest(sha) {
            pins.push((sha.to_ascii_lowercase(), url.to_string()));
        }
    }
    pins
}

/// `key: "value"` as the text of a recipe's declaration holds it.
fn quoted_field<'a>(block: &'a str, key: &str) -> Option<&'a str> {
    let at = block.find(&format!("{key}:"))?;
    let rest = block[at + key.len() + 1..].trim_start().strip_prefix('"')?;
    rest.split('"').next()
}

/// The `url`/`sha256` pairs of the recipe's own `SOURCES` for `libc`: every
/// `#{ ... }` block with an `https://` `url` and a 64-digit `sha256`; a block
/// with a `libc` field counts only for that C library.
fn sources_of(text: &str, libc: &str) -> Vec<(String, String)> {
    let text: String = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .flat_map(|line| [line, "\n"])
        .collect();
    let mut pins = Vec::new();
    for block in text.split("#{").skip(1) {
        let block = block.split('}').next().unwrap_or("");
        let (Some(url), Some(sha)) = (quoted_field(block, "url"), quoted_field(block, "sha256"))
        else {
            continue;
        };
        if url.starts_with("https://")
            && is_digest(sha)
            && quoted_field(block, "libc").is_none_or(|only| only == libc)
        {
            pins.push((sha.to_ascii_lowercase(), url.to_string()));
        }
    }
    pins
}

fn libc_name(libc: Libc) -> &'static str {
    match libc {
        Libc::Glibc => "glibc",
        Libc::Musl => "musl",
    }
}

/// A part may not be larger than this (GitHub's asset limit is 2 GiB; the
/// installer needs room to download and unpack).
pub(crate) const MAX_PART_BYTES: u64 = 1536 * 1024 * 1024;

/// The file name of the part of `category`, without the `.tar.zst`.
pub(crate) fn part_name(category: &str) -> String {
    format!("parts-{category}")
}

/// Puts the default set onto the release medium's `overlay`: its list and its
/// recipes with their deps. The package cache of every file they pin does not
/// go there: it is written into `parts_dir` as one `parts-<category>.tar.zst`
/// per category of the variant, each cache file in the first category (in the
/// contract's order) whose recipes, with their deps, need it.
pub(crate) fn write_medium(variant: &Variant, overlay: &Path, parts_dir: &Path) -> Result<()> {
    let repo = &variant.repo_root;
    let id = &variant.id;
    let set = &variant.contract.desktop.recipes;
    let all = closure(repo, set)?;
    let libc = libc_name(variant.contract.profile.libc);

    // The packages the group recipes pin are the desktop lock's: its preseed
    // puts them into the download store, where the medium takes them from.
    let preseed = preseed_recipe(variant, Flavour::Desktop);
    rootfs_source::preseed_rootfs_source(repo, id, &preseed, Flavour::Desktop, false)
        .with_context(|| format!("acquiring the desktop lock for the {id} release medium"))?;

    let work = crate::recipe::recipe_work_dir(repo, id)?.join("release-medium");
    let inputs = work.join("inputs");
    if inputs.exists() {
        fs::remove_dir_all(&inputs).with_context(|| format!("clearing {}", inputs.display()))?;
    }
    let recipes = inputs.join("recipes");
    fs::create_dir_all(&recipes).with_context(|| format!("creating {}", recipes.display()))?;
    let mut pins = BTreeSet::new();
    // What each recipe pins itself, for cutting the cache into parts.
    let mut pinned_by: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for name in &all {
        let file = recipe_file(repo, name);
        let text = fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        let own = pinned_by.entry(name).or_default();
        let found = sources_of(&text, libc);
        own.extend(found.iter().map(|(sha, _)| sha.clone()));
        pins.extend(found);
        fs::write(recipes.join(format!("{name}.rhai")), &text)
            .with_context(|| format!("copying recipes/{name}.rhai"))?;
        // The pins file travels with its recipe onto the medium.
        let pins_path = pins_file(repo, name);
        if pins_path.is_file() {
            let text = fs::read_to_string(&pins_path)
                .with_context(|| format!("reading {}", pins_path.display()))?;
            let found = pins_file_of(&text, libc);
            own.extend(found.iter().map(|(sha, _)| sha.clone()));
            pins.extend(found);
            fs::write(recipes.join(format!("{name}.pins")), &text)
                .with_context(|| format!("copying recipes/{name}.pins"))?;
        }
        // The modules it imports and the base it extends travel with it.
        for source in crate::update_release::recipe_files(variant, name)? {
            let file_name = source.file_name().context("a recipe file has a name")?;
            let target = recipes.join(file_name);
            if !target.exists() {
                fs::copy(&source, &target)
                    .with_context(|| format!("copying {} into the medium's recipes", source.display()))?;
            }
        }
    }
    let pins_file = inputs.join("pins");
    let text: String = pins.iter().map(|(sha, url)| format!("{sha} {url}\n")).collect();
    fs::write(&pins_file, text).with_context(|| format!("writing {}", pins_file.display()))?;
    let list = inputs.join("list");
    fs::write(&list, set.iter().map(|name| format!("{name}\n")).collect::<String>())
        .with_context(|| format!("writing {}", list.display()))?;

    let digests: BTreeSet<String> = pins.iter().map(|(sha, _)| sha.clone()).collect();
    let store = rootfs_source::store_view(repo, &digests)?;
    let medium = Invocation::new(repo, "install", repo.join(MEDIUM_RECIPE), work.join("recipe"))
        .input("pins", &pins_file)
        .input("recipes", &recipes)
        .input("list", &list)
        .input("store", store.path())
        .define("DEFAULT_SET_DIR", install_tools::DEFAULT_SET_DIR)
        .run()
        .with_context(|| format!("assembling the {id} release medium's default set"))?;
    drop(store);
    let (medium_tree, cache) = (medium.directory("medium")?, medium.directory("cache")?);
    levitate_recipe::target::merge_tree(&medium_tree, overlay)
        .with_context(|| format!("adding the default set to the overlay {}", overlay.display()))?;

    // Every cache file into exactly one part: the first category that needs it.
    let mut assigned = BTreeSet::new();
    for category in &variant.contract.desktop.categories {
        let mut files = BTreeSet::new();
        for name in closure(repo, &category.recipes)? {
            let own = pinned_by.get(name.as_str()).with_context(|| format!("the closure of {} has no recipe {name}", category.name))?;
            files.extend(own.iter().filter(|sha| !assigned.contains(*sha)).cloned());
        }
        assigned.extend(files.iter().cloned());
        write_part(id, &cache, parts_dir, &category.name, &files)?;
    }
    ensure!(
        assigned.len() == digests.len(),
        "the categories cover {} of the {} package files the default set pins; every recipe of [desktop].recipes must be in a category",
        assigned.len(),
        digests.len()
    );
    Ok(())
}

/// Writes `parts-<category>.tar.zst` into `parts_dir`: the cache files `files`
/// (SHA-256 digests) of the tree `cache`, as `sha256/<digest>` members after a
/// `sha256` directory, sorted, regular files 0644, owner root, mtime 0, so the
/// same files give the same bytes. Fails above [`MAX_PART_BYTES`].
fn write_part(distro: &str, cache: &Path, parts_dir: &Path, category: &str, files: &BTreeSet<String>) -> Result<()> {
    let name = part_name(category);
    let path = parts_dir.join(format!("{name}.tar.zst"));
    crate::tprintln!("[release] writing {name}.tar.zst ({} package files, zstd -19)", files.len());
    crate::update_release::compress_into(&path, |out| {
        let mut tar = tar::Builder::new(out);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_mode(0o755);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_size(0);
        tar.append_data(&mut header, "sha256", std::io::empty())?;
        for sha in files {
            crate::update_release::append_file(&mut tar, &format!("sha256/{sha}"), &cache.join("sha256").join(sha))?;
        }
        tar.finish()?;
        Ok(())
    })?;
    let size = fs::metadata(&path).with_context(|| format!("reading {}", path.display()))?.len();
    if size > MAX_PART_BYTES {
        let _ = fs::remove_file(&path);
        bail!(
            "{name}.tar.zst is {:.2} GiB, over the {:.1} GiB a part may have (a release asset is limited to 2 GiB); \
             split the category '{category}' in [[desktop.categories]] of distro/variants/{distro}/distro.toml into two \
             categories and build again",
            size as f64 / 1073741824.0,
            MAX_PART_BYTES as f64 / 1073741824.0,
        );
    }
    crate::tprintln!("[release] {name}.tar.zst: {:.1} MiB", size as f64 / 1048576.0);
    Ok(())
}

/// The account files, as Recipe's system route names them.
const ACCOUNT_FILES: [(&str, u32); 4] =
    [("etc/passwd", 0o644), ("etc/group", 0o644), ("etc/shadow", 0o600), ("etc/gshadow", 0o600)];

/// Puts the `network` group and its deps, built for the system `rootfs`, into
/// the live `overlay` the way `recipe --system --root` writes them into an
/// installed root: one producer run of `network` (each recipe of its closure
/// produced once, as the system route's producer does), its `system_producers`
/// read with Recipe's own `system::payloads`, and owner names and declared
/// accounts resolved with Recipe's `system::resolve_owners` against the live
/// system's account files (the overlay's where the live session already
/// replaced one, the base's otherwise). The trees go into the overlay with the
/// resolved ids; the account files go into it only when accounts were created.
/// Each group's `configure` (NetworkManager enabled) ran in its producer and
/// is in its tree. Not done here, on purpose: the receipt (the live session is
/// not an installed root; recsetup installs the default set into the target
/// with its own receipt) and the root's effects recipe (its caches are made
/// for the whole installed root; NetworkManager and nmcli need none of them,
/// and glibc finds the groups' libraries in the default folders without the
/// library cache).
pub(crate) fn add_live_network(variant: &Variant, rootfs: &Path, overlay: &Path) -> Result<()> {
    use levitate_recipe::{system, target};
    let repo = &variant.repo_root;
    let work = crate::recipe::recipe_work_dir(repo, &variant.id)?.join("live-groups");
    let produce = |redo: bool| {
        Invocation::new(repo, "install", recipe_file(repo, LIVE_NETWORK), work.join(LIVE_NETWORK))
            .recipes_path(recipes_dir(repo))
            .env(levitate_recipe::clibrary::ROOT_ENV, rootfs)
            // The system route's producer context: the result lists every
            // tree of the closure (`system_producers`).
            .define("RECIPE_SYSTEM_CONTEXT", "root")
            .redo(redo)
            .run()
            .with_context(|| format!("building the {LIVE_NETWORK} group for the {} live medium", variant.id))
    };
    let mut result = produce(false)?;
    // A reused result names its dependencies' trees too; one gone since is made again.
    let gone = result.json()["system_producers"].as_array().into_iter().flatten().any(|producer| {
        producer["outputs"]["system"].as_str().is_none_or(|path| !Path::new(path).is_dir())
    });
    if gone {
        result = produce(true)?;
    }
    fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
    let mut payloads = system::payloads(result.json(), "root")
        .with_context(|| format!("reading the {LIVE_NETWORK} group's system trees"))?;

    // The live system's account files, where Recipe resolves owners and makes accounts.
    let view = tempfile::tempdir_in(&work).with_context(|| format!("creating a folder under {}", work.display()))?;
    fs::create_dir_all(view.path().join("etc"))?;
    let mut before = Vec::new();
    for (name, _) in ACCOUNT_FILES {
        let from = [overlay.join(name), rootfs.join(name)].into_iter().find(|p| p.is_file());
        let text = match from {
            Some(path) => fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
            None => String::new(),
        };
        if !text.is_empty() {
            fs::write(view.path().join(name), &text)?;
        }
        before.push(text);
    }
    let accounts = system::resolve_owners(view.path(), &mut payloads)
        .with_context(|| format!("resolving the {LIVE_NETWORK} group's owners and accounts for the live medium"))?;

    for payload in &payloads {
        let name = &payload.package.name;
        target::merge_tree(&payload.root, overlay)
            .with_context(|| format!("adding the {name} group to the live overlay {}", overlay.display()))?;
        let mut manifest = target::load(overlay)?;
        for (path, resolved) in payload.package.files.iter().chain(&payload.seeds) {
            if let Some(entry) = manifest.entries.get_mut(path) {
                entry.uid = resolved.uid;
                entry.gid = resolved.gid;
                entry.user = None;
                entry.group = None;
            }
        }
        target::save(overlay, &manifest)?;
    }

    if let Some(after) = accounts {
        let base = target::load(rootfs)?;
        let files = tempfile::tempdir_in(&work)?;
        let texts = [&after.passwd, &after.group, &after.shadow, &after.gshadow];
        for ((name, default_mode), (text, old)) in ACCOUNT_FILES.iter().zip(texts.into_iter().zip(&before)) {
            if text == old {
                continue;
            }
            let path = files.path().join(name);
            fs::create_dir_all(path.parent().context("an account file has a folder")?)?;
            fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
            let mode = base.entries.get(*name).map_or(*default_mode, |entry| entry.mode);
            target::set_mode(files.path(), Path::new(name), mode)?;
        }
        target::merge_tree(files.path(), overlay)
            .with_context(|| format!("adding the live accounts to the overlay {}", overlay.display()))?;
    }
    Ok(())
}
