//! TUI apps on an image: one compiled binary holding every app a layer carries, and a
//! command per app.
//!
//! `bun build --compile` embeds the bun runtime, so each binary is tens of megabytes; one
//! binary per layer keeps that to a single copy. It runs the app named by the command it
//! was started as (`argv[0]`), like busybox. The runtime is the one for the target's
//! libc and CPU, read from the dynamic loader the target rootfs has, so a musl rootfs
//! (AcornOS) gets a musl build and nothing from the build host's bun lands on the image.

use anyhow::{bail, Context, Result};
use distro_contract::tui_apps::TuiApp;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Where the commands go, relative to a layer's root: `usr/local/bin/<app name>`.
pub const BIN_DIR: &str = "usr/local/bin";
/// The binary the commands link to.
const BINARY: &str = "usr/local/libexec/levitate-tui";

/// A bun compile target, and what it needs from the rootfs besides the loader.
struct Target {
    bun: &'static str,
    libs: &'static [&'static str],
}

/// Loaders by path in the rootfs, the bun target each means, and the shared libraries
/// that build links (bun's musl builds link libstdc++).
const TARGETS: &[(&str, Target)] = &[
    (
        "lib/ld-musl-x86_64.so.1",
        Target {
            bun: "bun-linux-x64-musl",
            libs: &["libstdc++.so.6"],
        },
    ),
    (
        "lib/ld-musl-aarch64.so.1",
        Target {
            bun: "bun-linux-arm64-musl",
            libs: &["libstdc++.so.6"],
        },
    ),
    (
        "lib64/ld-linux-x86-64.so.2",
        Target {
            bun: "bun-linux-x64",
            libs: &[],
        },
    ),
    (
        "lib/ld-linux-aarch64.so.1",
        Target {
            bun: "bun-linux-arm64",
            libs: &[],
        },
    ),
];

const LIB_DIRS: &[&str] = &["usr/lib", "usr/lib64", "lib", "lib64"];

/// Compiles `apps` into one binary in `layer` and links `BIN_DIR/<name>` to it for each.
/// `rootfs` is the root the binary will run on: it picks the target.
pub(crate) fn install(
    repo_root: &Path,
    rootfs: &Path,
    layer: &Path,
    apps: &[TuiApp],
) -> Result<()> {
    if apps.is_empty() {
        return Ok(());
    }
    let target = target_for(rootfs)?;
    let bun = crate::process::which("bun").context(
        "bun is required to build the TUI apps: install it and put it on PATH (https://bun.sh)",
    )?;

    // The apps are members of the root JavaScript workspace; install it the one way xtask does.
    run(
        Command::new(&bun)
            .args(["install", "--frozen-lockfile"])
            .current_dir(repo_root),
        "bun install --frozen-lockfile at the repo root",
    )?;

    let scratch = tempfile::tempdir().context("creating a directory for the TUI entry file")?;
    let entry = scratch.path().join("levitate-tui.ts");
    fs::write(&entry, entry_source(apps))
        .with_context(|| format!("writing '{}'", entry.display()))?;

    let binary = layer.join(BINARY);
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    // A target other than the host's makes bun download that runtime once (it caches it).
    run(
        Command::new(&bun)
            .arg("build")
            .arg(&entry)
            .args(["--compile", "--minify"])
            .arg(format!("--target={}", target.bun))
            .arg("--outfile")
            .arg(&binary),
        &format!("bun build --compile for {}", target.bun),
    )?;
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

/// The bun target for the loader `rootfs` has, once its libraries are checked.
fn target_for(rootfs: &Path) -> Result<&'static Target> {
    let (loader, target) = TARGETS
        .iter()
        .find(|(loader, _)| {
            // The merged-/usr path first: `lib64` may be a symlink that only resolves
            // inside the image.
            rootfs.join("usr").join(loader).exists() || rootfs.join(loader).exists()
        })
        .with_context(|| {
            format!(
                "no dynamic loader the TUI apps can run on in '{}' (looked for {})",
                rootfs.display(),
                TARGETS
                    .iter()
                    .map(|(l, _)| *l)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    for lib in target.libs {
        if !LIB_DIRS
            .iter()
            .any(|dir| rootfs.join(dir).join(lib).exists())
        {
            bail!(
                "the TUI apps' {} build needs {lib}, which '{}' lacks (its loader is {loader}): \
                 add the package that provides it to the rootfs",
                target.bun,
                rootfs.display()
            );
        }
    }
    Ok(target)
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

fn run(command: &mut Command, what: &str) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("running {what}"))?;
    if !output.status.success() {
        bail!(
            "{what} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}
