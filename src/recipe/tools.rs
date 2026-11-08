//! Explicit offline source preparation and managed producers for live tools.
use anyhow::{bail, Context, Result};
use distro_profile::Profile;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The live tools `recipes/install-tools.rhai` builds: package, crate directory,
/// binaries (the first is the package's default).
const TOOLS: &[(&str, &str, &[&str])] = &[
    ("recpart", "tools/recpart", &["recpart"]),
    (
        "recstrap",
        "tools/recstrap",
        &["recstrap", "recchroot", "recfstab", "recsetup"],
    ),
    ("recab", "tools/recab", &["recab"]),
    (
        "install-split-pane",
        "distro/builder/install-split-pane",
        &["install-split"],
    ),
];

fn tool(package: &str) -> Result<(&'static str, &'static str, &'static [&'static str])> {
    TOOLS
        .iter()
        .copied()
        .find(|(name, _, _)| *name == package)
        .with_context(|| format!("no live tool declared for package {package}"))
}

/// Prepare only the selected production crate and its local dependencies in a
/// temporary directory. Cargo resolves/prunes the copied workspace lock
/// offline; every selected dependency must remain an exact member of the
/// original lock.
pub fn prepare_tool_source(repo: &Path, package: &str) -> Result<tempfile::TempDir> {
    let source = tempfile::Builder::new()
        .prefix("recipe-tool-source-")
        .tempdir()?;
    prepare_tool_source_at(repo, package, source.path())?;
    Ok(source)
}

/// Prepare the source closure of `package` at `output`, which must not exist:
/// it is built beside `output` and renamed into place, so `output` is either
/// complete or absent.
pub fn prepare_tool_source_into(repo: &Path, package: &str, output: &Path) -> Result<()> {
    match fs::symlink_metadata(output) {
        Ok(_) => bail!("source output already exists: {}", output.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("checking '{}'", output.display()))
        }
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    let pending = tempfile::Builder::new()
        .prefix(".source-")
        .tempdir_in(parent)?;
    prepare_tool_source_at(repo, package, pending.path())?;
    let pending = pending.keep();
    if let Err(error) = fs::rename(&pending, output) {
        let _ = fs::remove_dir_all(&pending);
        return Err(error).with_context(|| format!("moving the source into '{}'", output.display()));
    }
    Ok(())
}

fn prepare_tool_source_at(repo: &Path, package: &str, source: &Path) -> Result<()> {
    let (_, relative, _) = tool(package)?;
    let relative = PathBuf::from(relative);
    let repo = repo.canonicalize()?;
    let root_manifest: toml::Value = toml::from_str(&fs::read_to_string(repo.join("Cargo.toml"))?)?;
    let root_workspace = root_manifest.get("workspace");
    let workspace_dependencies = root_workspace
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table);
    let mut copied = BTreeSet::new();
    copy_crate(&repo, &relative, source, workspace_dependencies, &mut copied)?;
    let mut workspace = toml::map::Map::new();
    workspace.insert("resolver".into(), "2".into());
    workspace.insert(
        "members".into(),
        vec![relative.to_string_lossy().to_string()].into(),
    );
    workspace.insert(
        "default-members".into(),
        vec![relative.to_string_lossy().to_string()].into(),
    );
    // Inherited package fields and dependencies (`x.workspace = true`) resolve
    // against the same tables as in the repository.
    for table in ["package", "dependencies"] {
        if let Some(value) = root_workspace.and_then(|w| w.get(table)) {
            workspace.insert(table.into(), value.clone());
        }
    }
    fs::write(
        source.join("Cargo.toml"),
        toml::to_string(&toml::Value::Table(
            [("workspace".into(), toml::Value::Table(workspace))]
                .into_iter()
                .collect(),
        ))?,
    )?;
    let original_lock = fs::read_to_string(repo.join("Cargo.lock"))?;
    fs::write(source.join("Cargo.lock"), &original_lock)?;
    let output = Command::new("cargo")
        .args(["vendor", "--offline", "--manifest-path"])
        .arg(source.join("Cargo.toml"))
        .arg(source.join("vendor"))
        .current_dir(&repo)
        .output()
        .context("preparing pinned offline Cargo source closure")?;
    if !output.status.success() {
        bail!(
            "cannot prepare offline source closure for {package}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let selected_lock = fs::read_to_string(source.join("Cargo.lock"))?;
    let unexpected: Vec<_> = locked_packages(&selected_lock)?
        .difference(&locked_packages(&original_lock)?)
        .cloned()
        .collect();
    if !unexpected.is_empty() {
        bail!("source closure selected dependencies outside the workspace lock: {unexpected:?}");
    }
    fs::create_dir_all(source.join(".cargo"))?;
    fs::write(source.join(".cargo/config.toml"),
        "[source.crates-io]\nreplace-with = \"vendored-sources\"\n[source.vendored-sources]\ndirectory = \"/inputs/source/vendor\"\n[net]\noffline = true\n")?;
    Ok(())
}

fn locked_packages(lock: &str) -> Result<BTreeSet<(String, String, String)>> {
    let lock: toml::Value = toml::from_str(lock)?;
    lock.get("package")
        .and_then(toml::Value::as_array)
        .context("Cargo lock has no package list")?
        .iter()
        .map(|package| {
            let field = |name| {
                package
                    .get(name)
                    .and_then(toml::Value::as_str)
                    .map(str::to_owned)
            };
            Ok((
                field("name").context("locked package has no name")?,
                field("version").context("locked package has no version")?,
                field("source").unwrap_or_default(),
            ))
        })
        .collect()
}

fn copy_crate(
    repo: &Path,
    relative: &Path,
    destination: &Path,
    workspace_dependencies: Option<&toml::map::Map<String, toml::Value>>,
    copied: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let original = repo.join(relative).canonicalize()?;
    let relative = original
        .strip_prefix(repo)
        .context("local tool dependency is outside the selected repository")?;
    if !copied.insert(relative.to_owned()) {
        return Ok(());
    }
    let manifest_path = original.join("Cargo.toml");
    let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(&manifest_path)?)?;
    let table = manifest
        .as_table_mut()
        .context("crate manifest must be a table")?;
    table.remove("dev-dependencies");
    table.remove("test");
    table.remove("bench");
    table.remove("example");
    let mut local_dependencies = Vec::new();
    collect_local_dependencies(
        table,
        &original,
        repo,
        workspace_dependencies,
        &mut local_dependencies,
    )?;
    let output = destination.join(relative);
    copy_crate_files(&original, &output)?;
    fs::write(output.join("Cargo.toml"), toml::to_string(&manifest)?)?;
    for dependency in local_dependencies {
        let dependency = dependency.canonicalize()?;
        let relative = dependency
            .strip_prefix(repo)
            .context("tool dependency is outside repository")?;
        copy_crate(repo, relative, destination, workspace_dependencies, copied)?;
    }
    Ok(())
}

/// Copy every file of the crate that git knows of (tracked, or new and not
/// ignored), so whatever the crate compiles from (`src`, a build script and
/// what it reads, `#[path]` modules, included files) comes along.
fn copy_crate_files(original: &Path, output: &Path) -> Result<()> {
    let listing = Command::new("git")
        .arg("-C")
        .arg(original)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
        .with_context(|| format!("listing the files of '{}' with git", original.display()))?;
    if !listing.status.success() {
        bail!(
            "cannot list the files of tool crate '{}' (it must be in a git checkout): {}",
            original.display(),
            String::from_utf8_lossy(&listing.stderr).trim()
        );
    }
    for name in listing.stdout.split(|byte| *byte == 0).filter(|name| !name.is_empty()) {
        let name = Path::new(std::str::from_utf8(name).context("tool crate file name is not UTF-8")?);
        let source = original.join(name);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            // Tracked but deleted in the working tree: the crate no longer has it.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error).with_context(|| format!("reading '{}'", source.display())),
        };
        let target = output.join(name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        if metadata.file_type().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(&source)?, &target)?;
        } else {
            fs::copy(&source, &target)
                .with_context(|| format!("copying '{}'", source.display()))?;
        }
    }
    Ok(())
}

fn collect_local_dependencies(
    table: &mut toml::map::Map<String, toml::Value>,
    original: &Path,
    repo: &Path,
    workspace_dependencies: Option<&toml::map::Map<String, toml::Value>>,
    dependencies: &mut Vec<PathBuf>,
) -> Result<()> {
    for name in ["dependencies", "build-dependencies"] {
        if let Some(entries) = table.get(name).and_then(toml::Value::as_table) {
            for (dependency_name, dependency) in entries {
                if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                    dependencies.push(original.join(path));
                }
                if dependency.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    // Resolved against the root's [workspace.dependencies], copied as it is.
                    let inherited = workspace_dependencies
                        .and_then(|deps| deps.get(dependency_name))
                        .with_context(|| {
                            format!("{dependency_name} inherits a workspace dependency the root manifest does not declare")
                        })?;
                    if let Some(path) = inherited.get("path").and_then(toml::Value::as_str) {
                        dependencies.push(repo.join(path));
                    }
                }
            }
        }
    }
    if let Some(targets) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
        for (_, target) in targets.iter_mut() {
            let target = target
                .as_table_mut()
                .context("target dependencies must be a table")?;
            target.remove("dev-dependencies");
            collect_local_dependencies(target, original, repo, workspace_dependencies, dependencies)?;
        }
    }
    Ok(())
}

/// Compile `package`'s `binary` (its default when `None`) through its recipe
/// for the image `profile` describes, and return the published package tree.
pub(crate) fn build_tool_tree(
    repo: &Path,
    package: &str,
    binary: Option<&str>,
    destination: Option<&str>,
    profile: &Profile,
) -> Result<PathBuf> {
    let (_, _, binaries) = tool(package)?;
    let binary = binary.unwrap_or(binaries[0]);
    if !binaries.contains(&binary) {
        bail!("package {package} does not build binary {binary}");
    }
    let target = profile.libc.rust_target();
    let init = profile.init.as_str();
    let source = prepare_tool_source(repo, package)?;
    let recipe = repo
        .join("distro/builder/recipes")
        .join("install-tools.rhai");
    let recipe_bin = super::find_recipe(repo)?;
    // The tree depends on the target (libc) and, for recab's trial service, the init system.
    let work = repo
        .join(".artifacts/work/recipe-tools")
        .join(binary)
        .join(target)
        .join(init);
    let install_path = destination
        .map(str::to_string)
        .unwrap_or_else(|| format!("usr/bin/{binary}"));
    let ctx = super::run_recipe_phase_json_with_inputs(
        &recipe_bin.path,
        "install",
        &recipe,
        &work,
        &[
            ("RUST_TARGET", target),
            ("BIN", binary),
            ("INSTALL_PATH", &install_path),
            ("INIT_SYSTEM", init),
        ],
        &[("source", source.path())],
        &[],
        None,
        recipe.parent(),
    )?;
    super::outputs::directory(&ctx, "rootfs")
}
