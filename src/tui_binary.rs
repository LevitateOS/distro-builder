//! TUI apps on an image: one compiled binary holding every app a layer carries, and a
//! command per app. The live install screen and the installed system each have their
//! own binary, so one layer never shadows the other's.
//!
//! `bun build --compile` embeds the bun runtime, so each binary is tens of megabytes; one
//! binary per layer keeps that to a single copy. It runs the app named by the command it
//! was started as (`argv[0]`), like busybox.
//!
//! Rust selects the source closure, apps, target libc and image paths. The
//! `tui-binary` recipe owns frozen dependency acquisition, compilation and
//! publication; its `bun-toolchain` dependency owns the verified compiler and
//! target runtime. No host Bun installation or node_modules cache is required.

use crate::tui_apps::{TuiApp, Workspace};
use anyhow::{Context, Result, bail};
use distro_contract::install_ux::BIN_DIR;
use distro_contract::profile::Libc;
use distro_contract::{Profile, SYSTEM_TUI_APPS};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The install screen's binary (live-tools overlay).
pub(crate) const INSTALL_BINARY: &str = "usr/local/libexec/install-tui";
/// The installed system's binary (system rootfs).
pub(crate) const SYSTEM_BINARY: &str = "usr/local/libexec/system-tui";

/// The one file of the TUI binary's prebuilt, and the recipe's output of that name.
const PROGRAM: &str = "program";

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
    let stem = Path::new(binary_path)
        .file_name()
        .and_then(|name| name.to_str())
        .context("the TUI binary has a file name")?;
    let work = repo_root.join(".artifacts/work").join(stem).join(target);
    fs::create_dir_all(&work)?;
    let source = prepare_source(repo_root, &work, apps)?;
    // The local cache, then a download, then this compile (see `crate::prebuilt`).
    // The key is taken before the target file joins the source tree.
    let obtained = crate::prebuilt::obtain(
        repo_root,
        crate::prebuilt::Family::Tui,
        stem,
        || tui_key(repo_root, stem, target, source.path()),
        &[PROGRAM],
        || crate::prebuilt::source_commits(repo_root, source_dirs(repo_root).unwrap_or_default()),
        || {
            let target_input = source.path().join("runtime-target");
            fs::write(&target_input, format!("{target}\n"))?;
            let result = crate::recipe::Invocation::new(
                repo_root,
                "install",
                "tui-binary",
                work.join("recipe"),
            )
            .input("source", source.path())
            .input("runtime_target", &target_input)
            .input("bun-toolchain::runtime_target", &target_input)
            .run()
            .context("building the TUI layer through Recipe")?;
            Ok(vec![(PROGRAM.to_string(), result.file(PROGRAM)?)])
        },
    )?;
    let published = obtained.path(PROGRAM)?.to_path_buf();
    let binary = layer.join(binary_path);
    if let Some(parent) = binary.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&published, &binary)
        .with_context(|| format!("copying published TUI program '{}'", published.display()))?;
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;

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
    use crate::pipeline::live_tools::{LIVE_TOOLS, ToolLayer};
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
    install(
        repo_root,
        rootfs,
        rootfs,
        SYSTEM_BINARY,
        profile.libc,
        &apps,
    )
    .context("building the installed system's TUI apps")
}

/// The key of the TUI binary `stem` (`install-tui` or `system-tui`) for `libc`'s
/// target: its source closure, the target, both recipes (`bun-toolchain` pins the
/// bun version), the compiler image and Recipe's declared behaviour.
fn tui_key(repo_root: &Path, stem: &str, target: &str, source: &Path) -> Result<crate::prebuilt::Key> {
    let recipes = repo_root.join("distro/builder/os-recipes");
    Ok(crate::prebuilt::Key::new(&[
        ("family", "tui".into()),
        ("name", stem.into()),
        ("target", target.into()),
        ("source_closure", crate::prebuilt::dir_digest(source)?),
        ("tui_binary_recipe", crate::prebuilt::file_digest(&recipes.join("tui-binary.rhai"))?),
        ("bun_toolchain_recipe", crate::prebuilt::file_digest(&recipes.join("bun-toolchain.rhai"))?),
        ("compiler_image", crate::recipe::compiler_image_reference(repo_root)?),
        ("declared", levitate_recipe::identity::declared()),
    ]))
}

/// `distro-builder prebuilt key tui <stem> --distro <id>`: prepares the source and
/// computes the key. `stem` is the binary's file name, `install-tui` or `system-tui`.
pub fn prebuilt_key(repo_root: &Path, stem: &str, libc: Libc) -> Result<crate::prebuilt::Key> {
    let workspace = Workspace::load(repo_root)?;
    let apps: Vec<&TuiApp> = if stem == file_stem(INSTALL_BINARY) {
        crate::install_ux::Screen::resolve(&workspace)?.apps()
    } else if stem == file_stem(SYSTEM_BINARY) {
        SYSTEM_TUI_APPS
            .iter()
            .map(|name| workspace.app(name))
            .collect::<Result<_>>()?
    } else {
        bail!(
            "unknown TUI binary '{stem}'; expected {} or {}",
            file_stem(INSTALL_BINARY),
            file_stem(SYSTEM_BINARY)
        );
    };
    let work = repo_root.join(".artifacts/work/prebuilt-key");
    fs::create_dir_all(&work)?;
    let source = prepare_source(repo_root, &work, &apps)?;
    tui_key(repo_root, stem, libc.bun_target(), source.path())
}

fn file_stem(binary_path: &str) -> &str {
    Path::new(binary_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(binary_path)
}

/// The checkout directories the TUI source is read from (the workspace members).
fn source_dirs(repo_root: &Path) -> Result<Vec<std::path::PathBuf>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(repo_root.join("package.json"))?)?;
    Ok(manifest
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| member.as_str().map(std::path::PathBuf::from))
                .collect()
        })
        .unwrap_or_default())
}

/// A plain source snapshot: workspace manifests and authored files, without
/// host dependency/build caches. Compiler/package facts remain in the recipes.
fn prepare_source(repo_root: &Path, work: &Path, apps: &[&TuiApp]) -> Result<tempfile::TempDir> {
    let source = tempfile::Builder::new()
        .prefix("tui-source-")
        .tempdir_in(work)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(repo_root.join("package.json"))?)?;
    for file in ["package.json", "bun.lock"] {
        let path = repo_root.join(file);
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            bail!("TUI source {file} must be a regular file");
        }
        fs::copy(path, source.path().join(file))?;
    }
    let members = manifest
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .context("the JavaScript workspace declares explicit member paths")?;
    for member in members {
        let member = member
            .as_str()
            .context("workspace member is a path string")?;
        let relative = Path::new(member);
        if relative.as_os_str().is_empty()
            || !relative
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
        {
            bail!("TUI source needs explicit relative workspace paths: {member}");
        }
        let root = repo_root.join(relative);
        if !fs::symlink_metadata(&root)?.file_type().is_dir() {
            bail!("workspace member {member} must be a real directory");
        }
        // Exactly what git lists: tracked files plus untracked ones that are not
        // ignored. Generated, ignored files (docs/content's src/generated) are
        // regenerated by tui-binary.rhai, so a fresh checkout, CI and this
        // machine compile, and key, the same source.
        let listed = std::process::Command::new("git")
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .current_dir(&root)
            .output()
            .with_context(|| format!("listing the files of workspace member {member} with git"))?;
        if !listed.status.success() {
            bail!(
                "git ls-files failed in workspace member {member}: {}",
                String::from_utf8_lossy(&listed.stderr)
            );
        }
        fs::create_dir_all(source.path().join(relative))?;
        for name in listed.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
            let path = root.join(std::str::from_utf8(name).context("non-UTF-8 TUI source path")?);
            // A tracked file deleted in the worktree is not part of the source.
            let Ok(metadata) = fs::symlink_metadata(&path) else { continue };
            let target = source.path().join(path.strip_prefix(repo_root)?);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            let entry = (path.as_path(), metadata.file_type());
            if entry.1.is_file() {
                fs::copy(entry.0, &target)?;
            } else if entry.1.is_symlink() {
                // Keep authored links (including fixture command aliases) as
                // links; never read through them into host files or caches.
                let link = fs::read_link(entry.0)?;
                let resolved = entry.0.canonicalize()?;
                if link.is_absolute() || !resolved.starts_with(root.canonicalize()?) {
                    bail!(
                        "TUI source link escapes its workspace member: '{}'",
                        entry.0.display()
                    );
                }
                std::os::unix::fs::symlink(link, target)?;
            } else {
                bail!("unsupported TUI source entry '{}'", entry.0.display());
            }
        }
    }
    fs::write(
        source.path().join("entry.ts"),
        entry_source(repo_root, repo_root, apps)?,
    )?;
    Ok(source)
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
    let up = if up.is_empty() { "." } else { &up };
    let mut source = String::from(
        "// Written by distro-builder (tui_binary.rs).\nimport { basename } from \"node:path\";\n\n\
         const apps: Record<string, () => Promise<unknown>> = {\n",
    );
    for app in apps {
        let path = app.dir.join(&app.entry);
        let relative = path.strip_prefix(repo_root).with_context(|| {
            format!(
                "the TUI app '{}' is outside '{}'",
                path.display(),
                repo_root.display()
            )
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
