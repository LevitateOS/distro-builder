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
//!   recipes with their deps (`install_tools::DEFAULT_SET_DIR`), and the
//!   package cache (`install_tools::RECIPE_CACHE_DIR`) holding every file
//!   those recipes pin for the variant's C library, taken from the download
//!   store that the desktop lock's preseed fills (`os-recipes/release-medium.rhai`
//!   assembles the tree).
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

use anyhow::{bail, Context, Result};
use distro_contract::profile::{Flavour, Libc};
use distro_contract::Variant;
use std::collections::BTreeSet;
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

/// Puts the default set onto the release medium's `overlay`: its list, its
/// recipes with their deps, and the package cache of every file they pin.
pub(crate) fn write_medium(variant: &Variant, overlay: &Path) -> Result<()> {
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
    for name in &all {
        let file = recipe_file(repo, name);
        let text = fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        pins.extend(sources_of(&text, libc));
        fs::write(recipes.join(format!("{name}.rhai")), &text)
            .with_context(|| format!("copying recipes/{name}.rhai"))?;
        // The pins file travels with its recipe onto the medium.
        let pins_path = pins_file(repo, name);
        if pins_path.is_file() {
            let text = fs::read_to_string(&pins_path)
                .with_context(|| format!("reading {}", pins_path.display()))?;
            pins.extend(pins_file_of(&text, libc));
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
        .define("CACHE_DIR", install_tools::RECIPE_CACHE_DIR)
        .define("DEFAULT_SET_DIR", install_tools::DEFAULT_SET_DIR)
        .run()
        .with_context(|| format!("assembling the {id} release medium's default set"))?
        .directory("medium")?;
    drop(store);
    levitate_recipe::target::merge_tree(&medium, overlay)
        .with_context(|| format!("adding the default set to the overlay {}", overlay.display()))
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
