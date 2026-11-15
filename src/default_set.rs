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
//!   deps, so the live session has NetworkManager. The builder runs those
//!   group recipes for the variant's system (`RECIPE_HOST_LIBC_ROOT`, the
//!   base tree) and merges their system trees into the overlay; their own
//!   configuration enables NetworkManager.
//!
//! Rust reads the recipes' declarations (`let deps`, the pins) and lays out
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

/// The names in a recipe's literal `let deps = [...]` line (none without one).
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

/// Every `[.., URL, .., SHA-256, ..]` pin a recipe's text holds for `libc`:
/// a quoted `https://` URL followed by the next quoted 64-digit digest, also
/// across lines. Pins inside a `// BEGIN GENERATED PINS <libc>` block, or under
/// a `glibc:`/`musl:` key, count only for that C library; others for every one.
fn pins_of(text: &str, libc: &str) -> Vec<(String, String)> {
    let mut pins = Vec::new();
    let mut section: Option<String> = None;
    let mut url: Option<String> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(name) = trimmed.strip_prefix("// BEGIN GENERATED PINS ") {
            section = Some(name.trim().to_string());
            continue;
        }
        if trimmed.starts_with("// END GENERATED PINS") || trimmed == "};" {
            section = None;
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        for key in ["glibc:", "musl:"] {
            if trimmed.starts_with(key) {
                section = Some(key.trim_end_matches(':').to_string());
            }
        }
        for token in trimmed.split('"').skip(1).step_by(2) {
            if token.starts_with("https://") {
                url = Some(token.to_string());
            } else if is_digest(token) {
                if let Some(found) = url.take() {
                    if section.as_deref().is_none_or(|s| s == libc) {
                        pins.push((token.to_ascii_lowercase(), found));
                    }
                }
            }
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
        pins.extend(pins_of(&text, libc));
        fs::write(recipes.join(format!("{name}.rhai")), &text)
            .with_context(|| format!("copying recipes/{name}.rhai"))?;
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

/// Puts the `network` group and its deps, built for the system `rootfs`, into
/// the live `overlay`.
pub(crate) fn add_live_network(variant: &Variant, rootfs: &Path, overlay: &Path) -> Result<()> {
    let repo = &variant.repo_root;
    let work = crate::recipe::recipe_work_dir(repo, &variant.id)?.join("live-groups");
    for name in closure(repo, &[LIVE_NETWORK.to_string()])? {
        let tree = Invocation::new(repo, "install", recipe_file(repo, &name), work.join(&name))
            .recipes_path(recipes_dir(repo))
            .env(levitate_recipe::clibrary::ROOT_ENV, rootfs)
            .run()
            .with_context(|| format!("building the {name} group for the {} live medium", variant.id))?
            .directory("system")?;
        levitate_recipe::target::merge_tree(&tree, overlay)
            .with_context(|| format!("adding the {name} group to the live overlay {}", overlay.display()))?;
    }
    Ok(())
}
