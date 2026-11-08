//! TUI apps on an image: one compiled binary holding every app a layer carries, and a
//! command per app.
//!
//! `bun build --compile` embeds the bun runtime, so each binary is tens of megabytes; one
//! binary per layer keeps that to a single copy. It runs the app named by the command it
//! was started as (`argv[0]`), like busybox. The runtime is the one for the image's
//! libc (the variant's profile), so a musl image gets a musl build and nothing from the
//! build host's bun lands on the image.

use anyhow::{bail, Context, Result};
use crate::process::Cmd;
use distro_contract::profile::Libc;
use distro_contract::tui_apps::TuiApp;
use std::fs;
use std::path::Path;

/// Where the commands go, relative to a layer's root: `usr/local/bin/<app name>`.
pub const BIN_DIR: &str = "usr/local/bin";
/// The binary the commands link to.
const BINARY: &str = "usr/local/libexec/levitate-tui";

const LIB_DIRS: &[&str] = &["usr/lib", "usr/lib64", "lib", "lib64"];

/// Compiles `apps` into one binary in `layer` and links `BIN_DIR/<name>` to it for each.
/// `rootfs` is the root the binary will run on, with C library `libc`.
pub(crate) fn install(
    repo_root: &Path,
    rootfs: &Path,
    layer: &Path,
    libc: Libc,
    apps: &[TuiApp],
) -> Result<()> {
    if apps.is_empty() {
        return Ok(());
    }
    check_libs(rootfs, libc)?;
    let target = libc.bun_target();

    // The apps are members of the root JavaScript workspace; install it the one way xtask does.
    Cmd::new("bun")
        .args(["install", "--frozen-lockfile"])
        .dir(repo_root)
        .error_msg("bun install --frozen-lockfile at the repo root")
        .run()?;

    let scratch = tempfile::tempdir().context("creating a directory for the TUI entry file")?;
    let entry = scratch.path().join("levitate-tui.ts");
    fs::write(&entry, entry_source(apps))
        .with_context(|| format!("writing '{}'", entry.display()))?;

    let binary = layer.join(BINARY);
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    // A target other than the host's makes bun download that runtime once (it caches it).
    Cmd::new("bun")
        .arg("build")
        .arg(&entry)
        .args(["--compile", "--minify"])
        .arg(format!("--target={target}"))
        .arg("--outfile")
        .arg(&binary)
        .error_msg(format!("bun build --compile for {target}"))
        .run()?;
    if !binary.is_file() {
        bail!("bun build wrote no binary at '{}'", binary.display());
    }

    let bin_dir = layer.join(BIN_DIR);
    fs::create_dir_all(&bin_dir).with_context(|| format!("creating '{}'", bin_dir.display()))?;
    let link_target = format!("/{BINARY}");
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

/// The entry that runs the app named by `argv[0]`. Each app's entry starts it on import,
/// so only the chosen one is imported.
fn entry_source(apps: &[TuiApp]) -> String {
    let mut source = String::from(
        "// Written by distro-builder (tui_binary.rs).\nimport { basename } from \"node:path\";\n\n\
         const apps: Record<string, () => Promise<unknown>> = {\n",
    );
    for app in apps {
        source.push_str(&format!(
            "\t{}: () => import({}),\n",
            json_string(&app.name),
            json_string(&app.dir.join(&app.entry).to_string_lossy())
        ));
    }
    source.push_str(
        "};\n\nconst name = basename(process.argv0);\nconst app = apps[name];\nif (!app) {\n\
         \tconsole.error(`${name}: run this as one of: ${Object.keys(apps).join(\", \")}`);\n\
         \tprocess.exit(2);\n}\nawait app();\n",
    );
    source
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("a string serializes")
}
