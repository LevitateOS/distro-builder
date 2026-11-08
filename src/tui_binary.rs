//! TUI apps on an image: one compiled binary holding every app a layer carries, and a
//! command per app. The live install screen and the installed system each have their
//! own binary, so one layer never shadows the other's.
//!
//! `bun build --compile` embeds the bun runtime, so each binary is tens of megabytes; one
//! binary per layer keeps that to a single copy. It runs the app named by the command it
//! was started as (`argv[0]`), like busybox.
//!
//! The build's inputs are declared: the bun version ([`BUN_VERSION`], which the host's
//! bun must be), the runtime for the image's libc (the variant's profile), fetched once
//! into `.artifacts/cache` and checked against its pinned hash, so nothing from the
//! build host's bun lands on the image; the apps as the root JavaScript workspace has
//! them installed (the build never installs it); and an entry file at a fixed place in
//! the checkout that imports each app by relative path, so neither the checkout's nor
//! the temp dir's location reaches the binary.

use anyhow::{bail, Context, Result};
use crate::process::Cmd;
use crate::tui_apps::{TuiApp, Workspace};
use distro_contract::install_ux::BIN_DIR;
use distro_contract::profile::Libc;
use distro_contract::{Profile, SYSTEM_TUI_APPS};
use std::fs;
use std::path::{Path, PathBuf};

/// The install screen's binary (live-tools overlay).
pub(crate) const INSTALL_BINARY: &str = "usr/local/libexec/install-tui";
/// The installed system's binary (system rootfs).
pub(crate) const SYSTEM_BINARY: &str = "usr/local/libexec/system-tui";

/// The bun that builds the binary, and whose runtime it embeds.
const BUN_VERSION: &str = "1.4.2";
/// `bun-v<BUN_VERSION>`'s release archive of each runtime target, by SHA-256
/// (its `SHASUMS256.txt`).
const RUNTIMES: &[(&str, &str)] = &[
    ("bun-linux-x64", "36368faef7527875d5ffa52e53cd48021741f2a83eb6208a8dd64068d422a913"),
    ("bun-linux-x64-musl", "4835eca59d6da70f4674f5642f6e459dcadab773695b2ed9922d131057989742"),
];

const LIB_DIRS: &[&str] = &["usr/lib", "usr/lib64", "lib", "lib64"];

/// Compiles `apps` into one binary at `binary_path` (`INSTALL_BINARY` or `SYSTEM_BINARY`)
/// in `layer` and links `BIN_DIR/<name>` to it for each. `rootfs` is the root the
/// binary will run on, with C library `libc`.
pub(crate) fn install(
    repo_root: &Path,
    rootfs: &Path,
    layer: &Path,
    binary_path: &str,
    libc: Libc,
    apps: &[&TuiApp],
) -> Result<()> {
    if apps.is_empty() {
        return Ok(());
    }
    check_libs(rootfs, libc)?;
    let target = libc.bun_target();
    check_bun()?;
    if !repo_root.join("node_modules").is_dir() {
        bail!(
            "the TUI apps are built from the installed root JavaScript workspace, and \
             '{}' has none: run `bun install --frozen-lockfile` there (`cargo xtask check` does)",
            repo_root.display()
        );
    }
    let runtime = runtime(repo_root, target)?;

    // One entry per runtime target, at the same place in every checkout.
    let stem = Path::new(binary_path)
        .file_name()
        .and_then(|name| name.to_str())
        .context("the TUI binary has a file name")?;
    let entry_dir = repo_root.join(".artifacts/work").join(stem).join(target);
    fs::create_dir_all(&entry_dir)
        .with_context(|| format!("creating '{}'", entry_dir.display()))?;
    let entry_file = format!("{stem}.ts");
    let entry = entry_dir.join(&entry_file);
    fs::write(&entry, entry_source(repo_root, &entry_dir, apps)?)
        .with_context(|| format!("writing '{}'", entry.display()))?;

    let binary = layer.join(binary_path);
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    Cmd::new("bun")
        .arg("build")
        .arg(&entry_file)
        .args(["--compile", "--minify"])
        .arg(format!("--target={target}"))
        .arg("--compile-executable-path")
        .arg(&runtime)
        .arg("--outfile")
        .arg(&binary)
        .dir(&entry_dir)
        .error_msg(format!(
            "bun build --compile for {target} (if a module is missing, the workspace \
             install is stale: run `bun install --frozen-lockfile` at the repo root)"
        ))
        .run()?;
    if !binary.is_file() {
        bail!("bun build wrote no binary at '{}'", binary.display());
    }

    let bin_dir = layer.join(BIN_DIR);
    fs::create_dir_all(&bin_dir).with_context(|| format!("creating '{}'", bin_dir.display()))?;
    let link_target = format!("/{binary_path}");
    for app in apps {
        let link = bin_dir.join(&app.name);
        if link.symlink_metadata().is_ok() {
            fs::remove_file(&link).with_context(|| format!("replacing '{}'", link.display()))?;
        }
        std::os::unix::fs::symlink(&link_target, &link)
            .with_context(|| format!("linking '{}' to {link_target}", link.display()))?;
    }
    Ok(())
}

/// Puts the installed system's TUI apps (`SYSTEM_TUI_APPS`) into the system `rootfs`,
/// which every product builds on. Each app's tools must be in the rootfs itself.
pub(crate) fn install_system(repo_root: &Path, rootfs: &Path, profile: &Profile) -> Result<()> {
    use crate::pipeline::live_tools::{ToolLayer, LIVE_TOOLS};
    let workspace = Workspace::load(repo_root)?;
    let apps = SYSTEM_TUI_APPS
        .iter()
        .map(|name| workspace.app(name))
        .collect::<Result<Vec<_>>>()?;
    for app in &apps {
        for tool in &app.tools {
            let shipped = LIVE_TOOLS.iter().any(|live| {
                live.layer == ToolLayer::Rootfs && live.binary.unwrap_or(live.package) == tool
            });
            if !shipped {
                bail!(
                    "the installed system's app '{}' runs {tool}, but the system rootfs doesn't \
                     ship a binary called that: add it to the builder's rootfs tools \
                     (pipeline/live_tools.rs)",
                    app.name
                );
            }
        }
    }
    install(repo_root, rootfs, rootfs, SYSTEM_BINARY, profile.libc, &apps)
        .context("building the installed system's TUI apps")
}

/// The host's bun is the declared one: it writes the bundle the runtime runs.
fn check_bun() -> Result<()> {
    let version = Cmd::new("bun")
        .arg("--version")
        .error_msg("bun --version (the TUI apps are built with bun: https://bun.sh)")
        .run()?;
    let version = version.stdout.trim();
    if version != BUN_VERSION {
        bail!(
            "the TUI apps are built with bun {BUN_VERSION}, and the host's is {version}: \
             install bun {BUN_VERSION} (`bun upgrade --version {BUN_VERSION}` or \
             https://bun.sh), or move BUN_VERSION and its runtime hashes in tui_binary.rs"
        );
    }
    Ok(())
}

/// The bun runtime for `target`, fetched once from bun's release into
/// `.artifacts/cache/bun/<version>/` and checked against its pinned hash.
fn runtime(repo_root: &Path, target: &str) -> Result<PathBuf> {
    let Some((_, sha256)) = RUNTIMES.iter().find(|(name, _)| *name == target) else {
        bail!("no bun runtime for target {target} is pinned in tui_binary.rs");
    };
    let dir = repo_root.join(".artifacts/cache/bun").join(BUN_VERSION);
    let executable = dir.join(target).join("bun");
    if executable.is_file() {
        return Ok(executable);
    }
    fs::create_dir_all(&dir).with_context(|| format!("creating '{}'", dir.display()))?;
    let archive = dir.join(format!("{target}.zip"));
    let url = format!(
        "https://github.com/oven-sh/bun/releases/download/bun-v{BUN_VERSION}/{target}.zip"
    );
    Cmd::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error", "--output"])
        .arg(&archive)
        .arg(&url)
        .error_msg(format!("downloading {url}"))
        .run()?;
    let sums = Cmd::new("sha256sum")
        .arg(&archive)
        .error_msg("sha256sum of the bun runtime")
        .run()?;
    let actual = sums.stdout.split_whitespace().next().unwrap_or_default();
    if actual != *sha256 {
        fs::remove_file(&archive).ok();
        bail!("{url} has SHA-256 {actual}, not the pinned {sha256}");
    }
    // The archive holds `<target>/bun`.
    Cmd::new("unzip")
        .args(["-q", "-o"])
        .arg(&archive)
        .arg(format!("{target}/bun"))
        .arg("-d")
        .arg(&dir)
        .error_msg(format!("unpacking {}", archive.display()))
        .run()?;
    fs::remove_file(&archive).with_context(|| format!("removing '{}'", archive.display()))?;
    if !executable.is_file() {
        bail!("{url} holds no {target}/bun");
    }
    Ok(executable)
}

/// The shared libraries the bun build for `libc` links are in `rootfs`.
fn check_libs(rootfs: &Path, libc: Libc) -> Result<()> {
    for lib in libc.bun_libs() {
        if !LIB_DIRS
            .iter()
            .any(|dir| rootfs.join(dir).join(lib).exists())
        {
            bail!(
                "the TUI apps' {} build needs {lib}, which '{}' lacks: add the package that \
                 provides it to the rootfs",
                libc.bun_target(),
                rootfs.display()
            );
        }
    }
    Ok(())
}

/// The entry, written in `entry_dir`, that runs the app named by `argv[0]`. Each app's
/// entry starts it on import, so only the chosen one is imported.
fn entry_source(repo_root: &Path, entry_dir: &Path, apps: &[&TuiApp]) -> Result<String> {
    let up = entry_dir
        .strip_prefix(repo_root)
        .context("the TUI entry is inside the checkout")?
        .components()
        .map(|_| "..")
        .collect::<Vec<_>>()
        .join("/");
    let mut source = String::from(
        "// Written by distro-builder (tui_binary.rs).\nimport { basename } from \"node:path\";\n\n\
         const apps: Record<string, () => Promise<unknown>> = {\n",
    );
    for app in apps {
        let path = app.dir.join(&app.entry);
        let relative = path.strip_prefix(repo_root).with_context(|| {
            format!("the TUI app '{}' is outside '{}'", path.display(), repo_root.display())
        })?;
        source.push_str(&format!(
            "\t{}: () => import({}),\n",
            json_string(&app.name),
            json_string(&format!("{up}/{}", relative.display()))
        ));
    }
    source.push_str(
        "};\n\nconst name = basename(process.argv0);\nconst app = apps[name];\nif (!app) {\n\
         \tconsole.error(`${name}: run this as one of: ${Object.keys(apps).join(\", \")}`);\n\
         \tprocess.exit(2);\n}\nawait app();\n",
    );
    Ok(source)
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string serializes")
}
