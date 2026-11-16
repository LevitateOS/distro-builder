//! TUI apps on an image: one compiled binary holding every app the live install screen
//! carries, and a command per app.
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
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// The install screen's binary (test-install-base overlay).
pub(crate) const INSTALL_BINARY: &str = "usr/local/libexec/install-tui";

/// The one file of the TUI binary's prebuilt, and the recipe's output of that name.
const PROGRAM: &str = "program";

const LIB_DIRS: &[&str] = &["usr/lib", "usr/lib64", "lib", "lib64"];

/// Compiles `apps` into one binary at `binary_path` (`INSTALL_BINARY`)
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

/// The key of the TUI binary `stem` (`install-tui`) for `libc`'s
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
        ("behaviour", crate::prebuilt::behaviour()),
    ]))
}

/// `distro-builder prebuilt key tui <stem> --distro <id>`: prepares the source and
/// computes the key. `stem` is the binary's file name, `install-tui`.
pub fn prebuilt_key(repo_root: &Path, stem: &str, libc: Libc) -> Result<crate::prebuilt::Key> {
    let workspace = Workspace::load(repo_root)?;
    if stem != file_stem(INSTALL_BINARY) {
        bail!("unknown TUI binary '{stem}'; expected {}", file_stem(INSTALL_BINARY));
    }
    let apps: Vec<&TuiApp> = crate::install_ux::Screen::resolve(&workspace)?.apps();
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

/// The workspace members the entry for `apps` imports, transitively: the members holding
/// the apps, and every member a selected member's package.json names among its
/// dependencies (any kind: a dev or peer dependency may be imported for its types), by
/// package name. Paths relative to the checkout, as `workspaces` names them.
fn imported_members(repo_root: &Path, members: &[String], apps: &[&TuiApp]) -> Result<BTreeSet<String>> {
    let mut by_name = BTreeMap::new();
    for member in members {
        let path = repo_root.join(member).join("package.json");
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(&path).with_context(|| format!("reading '{}'", path.display()))?,
        )
        .with_context(|| format!("parsing '{}'", path.display()))?;
        if let Some(name) = manifest.get("name").and_then(serde_json::Value::as_str) {
            by_name.insert(name.to_string(), (member.clone(), manifest));
        }
    }
    let mut todo = Vec::new();
    for app in apps {
        let dir = app.dir.strip_prefix(repo_root).with_context(|| {
            format!("the TUI app '{}' is outside '{}'", app.dir.display(), repo_root.display())
        })?;
        let member = members
            .iter()
            .filter(|member| dir.starts_with(member.as_str()))
            .max_by_key(|member| member.len())
            .with_context(|| format!("the TUI app '{}' is in no workspace member", dir.display()))?;
        todo.push(member.clone());
    }
    let mut selected = BTreeSet::new();
    while let Some(member) = todo.pop() {
        if !selected.insert(member.clone()) {
            continue;
        }
        let Some((_, manifest)) = by_name.values().find(|(path, _)| *path == member) else { continue };
        for kind in ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"] {
            for name in manifest.get(kind).and_then(serde_json::Value::as_object).into_iter().flat_map(|deps| deps.keys()) {
                if let Some((path, _)) = by_name.get(name) {
                    todo.push(path.clone());
                }
            }
        }
    }
    Ok(selected)
}

/// Whether a file of an imported workspace member (relative to the member) is read by the
/// TUI's build: its package.json and tsconfig*.json at the member's root, its `src/`
/// (`bun build` follows imports there, `../package.json` included), and docs/content's
/// `scripts/` (tui-binary.rhai runs `scripts/build.ts`, which reads `src/` and imports
/// `scripts/syntax.ts`, to write `src/generated/index.ts`). Never READMEs, licences, agent
/// notes, fixtures, snapshots, CI files or a member's own lock.
fn member_file_read(member: &str, file: &Path) -> bool {
    let parts: Vec<_> = file.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    match parts.as_slice() {
        [name] => name == "package.json" || (name.starts_with("tsconfig") && name.ends_with(".json")),
        [top, _, ..] => top == "src" || (top == "scripts" && member == "docs/content"),
        [] => false,
    }
}

/// A plain source snapshot: the root workspace manifest and lock, and of each workspace
/// member only what the build reads: everything [`member_file_read`] keeps for the members
/// the entry imports ([`imported_members`]), and only the package.json of the others (bun's
/// `--frozen-lockfile` checks the lock against every member's manifest). No host
/// dependency/build caches. Compiler/package facts remain in the recipes. The key hashes this
/// same snapshot.
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
    let members: Vec<String> = manifest
        .get("workspaces")
        .and_then(serde_json::Value::as_array)
        .context("the JavaScript workspace declares explicit member paths")?
        .iter()
        .map(|member| member.as_str().map(str::to_owned).context("workspace member is a path string"))
        .collect::<Result<_>>()?;
    let imported = imported_members(repo_root, &members, apps)?;
    for member in &members {
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
        fs::create_dir_all(source.path().join(relative))?;
        if !imported.contains(member) {
            let manifest = root.join("package.json");
            if !fs::symlink_metadata(&manifest).is_ok_and(|m| m.is_file()) {
                bail!("workspace member {member} has no package.json");
            }
            fs::copy(&manifest, source.path().join(relative).join("package.json"))?;
            continue;
        }
        // Of what git lists (tracked files plus untracked ones that are not
        // ignored), what the build reads. Generated, ignored files
        // (docs/content's src/generated) are regenerated by tui-binary.rhai, so
        // a fresh checkout, CI and this machine compile, and key, the same source.
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
        for name in listed.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
            let name = std::str::from_utf8(name).context("non-UTF-8 TUI source path")?;
            if !member_file_read(member, Path::new(name)) {
                continue;
            }
            let path = root.join(name);
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
    // The key hashes modes: the same on every machine, whatever its umask.
    crate::prebuilt::normalize_modes(source.path())?;
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
