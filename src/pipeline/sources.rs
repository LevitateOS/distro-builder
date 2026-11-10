//! The sources a system rootfs is built from, as one digest. A product without
//! a parent builds the system from the checkout (the builder's recipes and
//! locks, the contract, the variant, the installed system's TUI apps and the
//! rootfs tools' crates) with every recipe step run by the Ginux launcher
//! built from `tools/ginux`; every other product carries its parent's rootfs
//! unchanged. Each release records the digest of the sources its rootfs was
//! built from, so a product is never built on a parent whose system is older
//! than the checkout: the planner rebuilds such a parent first, and preparing
//! a product refuses one.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::live_tools::{ToolLayer, LIVE_TOOLS};
use crate::process::Cmd;

/// The file in a release run's (or prepared product's) directory that holds
/// the digest of the sources its rootfs was built from.
pub const SOURCES_FILE: &str = "system-sources.sha256";

/// The digest of the sources `distro_id`'s system rootfs is built from, as the
/// checkout has them now: every file git lists (tracked, or new and not
/// ignored, as the tools' source closures take them) under each source, with
/// its path and content, and the Ginux launcher the checkout builds (Ginux is
/// developed here: a changed launcher rebuilds the system).
pub fn system_sources_digest(repo: &Path, distro_id: &str) -> Result<String> {
    let repo = repo.canonicalize()?;
    let mut roots: BTreeSet<PathBuf> = [
        "distro/builder",
        "distro/contract",
        "distro/profile",
        "tui",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();
    roots.insert(PathBuf::from("distro/variants").join(distro_id));
    for tool in LIVE_TOOLS.iter().filter(|tool| tool.layer == ToolLayer::Rootfs) {
        roots.extend(crate::recipe::tools::tool_crate_dirs(&repo, tool.package)?);
    }
    let mut hasher = Sha256::new();
    let ginux = crate::recipe::ginux_binary(&repo)?;
    hasher.update(b"ginux launcher\0");
    hasher.update(fs::read(ginux).with_context(|| format!("reading '{}'", ginux.display()))?);
    hasher.update([0]);
    // The workspace's manifest and locks pick the tools' and TUI's dependencies.
    for file in ["Cargo.toml", "Cargo.lock", "package.json", "bun.lock"] {
        hash_file(&mut hasher, &repo, Path::new(file))?;
    }
    for root in &roots {
        let listing = Cmd::new("git")
            .arg("-C")
            .arg(repo.join(root))
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .error_msg(format!("listing the files of '{}' (it must be in a git checkout)", root.display()))
            .run()?;
        let mut names: Vec<&str> = listing.stdout.split('\0').filter(|name| !name.is_empty()).collect();
        names.sort_unstable();
        for name in names {
            hash_file(&mut hasher, &repo, &root.join(name))?;
        }
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// `relative`'s path and content (a link's target) into `hasher`; a tracked
/// file deleted in the working tree is skipped, as the source closures skip it.
fn hash_file(hasher: &mut Sha256, repo: &Path, relative: &Path) -> Result<()> {
    let path = repo.join(relative);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("reading '{}'", path.display())),
    };
    hasher.update(relative.to_string_lossy().as_bytes());
    hasher.update([0]);
    if metadata.file_type().is_symlink() {
        hasher.update(b"link:");
        hasher.update(fs::read_link(&path)?.to_string_lossy().as_bytes());
    } else if metadata.is_file() {
        hasher.update(fs::read(&path).with_context(|| format!("reading '{}'", path.display()))?);
    }
    // A directory listed by git is a submodule's gitlink; its own files are
    // listed under its own root where it is a source.
    hasher.update([0]);
    Ok(())
}

/// The digest recorded in `dir` (a release run's or prepared product's
/// directory), if any.
pub fn recorded(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(SOURCES_FILE)).ok().map(|text| text.trim().to_string())
}

/// Records `digest` in `dir`.
pub fn record(dir: &Path, digest: &str) -> Result<()> {
    let path = dir.join(SOURCES_FILE);
    fs::write(&path, format!("{digest}\n")).with_context(|| format!("writing {}", path.display()))
}
