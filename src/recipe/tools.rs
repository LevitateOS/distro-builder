//! Explicit offline source preparation and managed producers for live tools.
use anyhow::{bail, Context, Result};
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
        &["recstrap", "recchroot", "recfstab"],
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

/// Prepare only the selected production crate and its local dependencies.
/// Cargo resolves/prunes the copied workspace lock offline; every selected
/// dependency must remain an exact member of the original lock.
pub fn prepare_tool_source(repo: &Path, package: &str) -> Result<tempfile::TempDir> {
    let (_, relative, _) = tool(package)?;
    let relative = PathBuf::from(relative);
    let repo = repo.canonicalize()?;
    let source = tempfile::Builder::new()
        .prefix("recipe-tool-source-")
        .tempdir()?;
    let mut copied = BTreeSet::new();
    copy_crate(&repo, &relative, source.path(), &mut copied)?;
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
    let root_manifest: toml::Value = toml::from_str(&fs::read_to_string(repo.join("Cargo.toml"))?)?;
    if let Some(package) = root_manifest
        .get("workspace")
        .and_then(|w| w.get("package"))
    {
        workspace.insert("package".into(), package.clone());
    }
    fs::write(
        source.path().join("Cargo.toml"),
        toml::to_string(&toml::Value::Table(
            [("workspace".into(), toml::Value::Table(workspace))]
                .into_iter()
                .collect(),
        ))?,
    )?;
    let original_lock = fs::read_to_string(repo.join("Cargo.lock"))?;
    fs::write(source.path().join("Cargo.lock"), &original_lock)?;
    let output = Command::new("cargo")
        .args(["vendor", "--offline", "--manifest-path"])
        .arg(source.path().join("Cargo.toml"))
        .arg(source.path().join("vendor"))
        .current_dir(&repo)
        .output()
        .context("preparing pinned offline Cargo source closure")?;
    if !output.status.success() {
        bail!(
            "cannot prepare offline source closure for {package}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let selected_lock = fs::read_to_string(source.path().join("Cargo.lock"))?;
    let unexpected: Vec<_> = locked_packages(&selected_lock)?
        .difference(&locked_packages(&original_lock)?)
        .cloned()
        .collect();
    if !unexpected.is_empty() {
        bail!("source closure selected dependencies outside the workspace lock: {unexpected:?}");
    }
    fs::create_dir_all(source.path().join(".cargo"))?;
    fs::write(source.path().join(".cargo/config.toml"),
        "[source.crates-io]\nreplace-with = \"vendored-sources\"\n[source.vendored-sources]\ndirectory = \"/inputs/source/vendor\"\n[net]\noffline = true\n")?;
    Ok(source)
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
    collect_local_dependencies(table, &original, &mut local_dependencies)?;
    let output = destination.join(relative);
    fs::create_dir_all(&output)?;
    crate::copy_dir_recursive(&original.join("src"), &output.join("src"))?;
    if let Some(build) = manifest
        .get("package")
        .and_then(|p| p.get("build"))
        .and_then(toml::Value::as_str)
    {
        let path = Path::new(build);
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            bail!("build script path escapes tool crate");
        }
        if let Some(parent) = output.join(path).parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(original.join(path), output.join(path))?;
    } else if original.join("build.rs").is_file() {
        fs::copy(original.join("build.rs"), output.join("build.rs"))?;
    }
    fs::write(output.join("Cargo.toml"), toml::to_string(&manifest)?)?;
    for dependency in local_dependencies {
        let dependency = dependency.canonicalize()?;
        let relative = dependency
            .strip_prefix(repo)
            .context("tool dependency is outside repository")?;
        copy_crate(repo, relative, destination, copied)?;
    }
    Ok(())
}

fn collect_local_dependencies(
    table: &mut toml::map::Map<String, toml::Value>,
    original: &Path,
    dependencies: &mut Vec<PathBuf>,
) -> Result<()> {
    for name in ["dependencies", "build-dependencies"] {
        if let Some(entries) = table.get(name).and_then(toml::Value::as_table) {
            for dependency in entries.values() {
                if let Some(path) = dependency.get("path").and_then(toml::Value::as_str) {
                    dependencies.push(original.join(path));
                }
                if dependency.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                    bail!("tool source producer requires explicit crate dependency declarations");
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
            collect_local_dependencies(target, original, dependencies)?;
        }
    }
    Ok(())
}

/// Compile through the package's actual recipe and return its published program.
pub fn build_tool_program(
    repo: &Path,
    package: &str,
    binary: Option<&str>,
    target: Option<&str>,
) -> Result<PathBuf> {
    let (_, _, binaries) = tool(package)?;
    let binary = binary.unwrap_or(binaries[0]);
    if !binaries.contains(&binary) {
        bail!("package {package} does not build binary {binary}");
    }
    // The recipe checks the target.
    let target = target.unwrap_or("x86_64-unknown-linux-gnu");
    let source = prepare_tool_source(repo, package)?;
    let recipe = repo
        .join("distro/builder/recipes")
        .join("install-tools.rhai");
    let recipe_bin = super::find_recipe(repo)?;
    let work = repo
        .join(".artifacts/work/recipe-tools")
        .join(binary)
        .join(target);
    let ctx = super::run_recipe_phase_json_with_inputs(
        &recipe_bin.path,
        "install",
        &recipe,
        &work,
        &[("RUST_TARGET", target), ("BIN", binary)],
        &[("source", source.path())],
        &[],
        None,
        recipe.parent(),
    )?;
    super::outputs::file(&ctx, "program")
}
