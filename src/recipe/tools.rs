//! Explicit offline source preparation and managed producers for live tools.
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::Cmd;

/// The live tools `os-recipes/install-tools.rhai` builds: package, crate directory,
/// binaries (the first is the package's default).
const TOOLS: &[(&str, &str, &[&str])] = &[
    ("recpart", "tools/recpart", &["recpart"]),
    (
        "recstrap",
        "tools/recstrap",
        &["recstrap", "recchroot", "recfstab", "recsetup"],
    ),
    ("recab", "tools/recab", &["recab"]),
    ("recguest", "tools/recguest", &["recguest"]),
    ("recupdate", "tools/recupdate", &["recupdate"]),
    ("levitate-recipe", "tools/recipe", &["recipe"]),
    (
        "install-split-pane",
        "distro/builder/install-split-pane",
        &["install-split"],
    ),
];

/// Every tool package with a prebuilt (`prebuilt list`).
pub fn tool_packages() -> Vec<&'static str> {
    TOOLS.iter().map(|(name, _, _)| *name).collect()
}

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
    prepare_crate_source_at(repo, Path::new(relative), &[], source)
        .with_context(|| format!("preparing the offline source closure for {package}"))
}

/// [`prepare_tool_source`] for any workspace crate, named by its directory
/// relative to the repository: the crate and its local dependencies (each
/// only its `Cargo.toml`, `src/` and what `src/` embeds, see
/// [`copy_crate_files`]), a one-member workspace that inherits the entries of
/// the root's `[workspace.package]` and `[workspace.dependencies]` those
/// crates use, the root's `Cargo.lock` pruned to the crate's closure, and
/// every crates.io dependency vendored under `vendor/` with
/// `.cargo/config.toml` pointing there (`/inputs/source/vendor`). Compiling it
/// needs no network: `cargo build --locked --offline`. `extra` names further
/// files of the crate itself (relative to its directory) that the recipe
/// reads. The login screen (`crate::login_screen`), which is not a static
/// tool, is prepared this way.
pub(crate) fn prepare_crate_source(repo: &Path, relative: &Path, extra: &[String]) -> Result<tempfile::TempDir> {
    let source = tempfile::Builder::new()
        .prefix("recipe-crate-source-")
        .tempdir()?;
    prepare_crate_source_at(repo, relative, extra, source.path())
        .with_context(|| format!("preparing the offline source closure of {}", relative.display()))?;
    Ok(source)
}

fn prepare_crate_source_at(repo: &Path, relative: &Path, extra: &[String], source: &Path) -> Result<()> {
    let repo = repo.canonicalize()?;
    let relative = repo
        .join(relative)
        .canonicalize()?
        .strip_prefix(&repo)
        .context("the crate is outside the selected repository")?
        .to_path_buf();
    let root_manifest: toml::Value = toml::from_str(&fs::read_to_string(repo.join("Cargo.toml"))?)?;
    let root_workspace = root_manifest.get("workspace");
    let workspace_dependencies = root_workspace
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table);
    let closure = resolve_closure(&repo, &relative, workspace_dependencies)?;
    for (dir, manifest) in &closure {
        let output = source.join(dir);
        copy_crate_files(&repo.join(dir), &output, if *dir == relative { extra } else { &[] })?;
        fs::write(output.join("Cargo.toml"), toml::to_string(manifest)?)?;
    }
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
    // against the same entries as in the repository; only the entries the
    // closure's crates inherit are carried, so an unrelated workspace
    // dependency changes no key.
    let (package_fields, dependency_names, lints) = inherited_workspace_entries(closure.values());
    for (table, wanted) in [("package", &package_fields), ("dependencies", &dependency_names)] {
        if let Some(entries) = root_workspace.and_then(|w| w.get(table)).and_then(toml::Value::as_table) {
            let kept: toml::map::Map<String, toml::Value> = entries
                .iter()
                .filter(|(name, _)| wanted.contains(*name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect();
            if let Some(missing) = wanted.iter().find(|name| !kept.contains_key(*name)) {
                bail!("a crate of the closure inherits workspace.{table}.{missing}, which the root manifest does not declare");
            }
            if !kept.is_empty() {
                workspace.insert(table.into(), toml::Value::Table(kept));
            }
        } else if let Some(missing) = wanted.iter().next() {
            bail!("a crate of the closure inherits workspace.{table}.{missing}, but the root manifest has no [workspace.{table}]");
        }
    }
    if lints {
        let value = root_workspace
            .and_then(|w| w.get("lints"))
            .context("a crate of the closure inherits [workspace.lints], which the root manifest does not declare")?;
        workspace.insert("lints".into(), value.clone());
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
    Cmd::new("cargo")
        .args(["vendor", "--offline", "--manifest-path"])
        .arg(source.join("Cargo.toml"))
        .arg(source.join("vendor"))
        .dir(&repo)
        .error_msg(format!("vendoring the crates.io dependencies of {}", relative.display()))
        .run()?;
    omit_vendor_development_files(&source.join("vendor"))?;
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
    // The key hashes modes: the same on every machine, whatever its umask.
    crate::prebuilt::normalize_modes(source)?;
    Ok(())
}

/// The `[workspace.package]` fields, `[workspace.dependencies]` names and
/// whether `[workspace.lints]` the `manifests` inherit (`x.workspace = true`).
fn inherited_workspace_entries<'a>(
    manifests: impl Iterator<Item = &'a toml::map::Map<String, toml::Value>>,
) -> (BTreeSet<String>, BTreeSet<String>, bool) {
    let inherits = |value: &toml::Value| value.get("workspace").and_then(toml::Value::as_bool) == Some(true);
    let (mut fields, mut names, mut lints) = (BTreeSet::new(), BTreeSet::new(), false);
    for manifest in manifests {
        if let Some(package) = manifest.get("package").and_then(toml::Value::as_table) {
            fields.extend(package.iter().filter(|(_, value)| inherits(value)).map(|(name, _)| name.clone()));
        }
        lints |= manifest.get("lints").is_some_and(inherits);
        for_each_dependency_table(manifest, &mut |_, entries| {
            names.extend(entries.iter().filter(|(_, value)| inherits(value)).map(|(name, _)| name.clone()));
        });
    }
    (fields, names, lints)
}

/// Calls `f` with every dependency table of `manifest` (`dependencies`,
/// `build-dependencies`, and both under each `target.<cfg>`), named by its
/// section.
fn for_each_dependency_table(
    manifest: &toml::map::Map<String, toml::Value>,
    f: &mut dyn FnMut(&str, &toml::map::Map<String, toml::Value>),
) {
    for section in ["dependencies", "build-dependencies"] {
        if let Some(entries) = manifest.get(section).and_then(toml::Value::as_table) {
            f(section, entries);
        }
    }
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for target in targets.values().filter_map(toml::Value::as_table) {
            for section in ["dependencies", "build-dependencies"] {
                if let Some(entries) = target.get(section).and_then(toml::Value::as_table) {
                    f(section, entries);
                }
            }
        }
    }
}

/// [`for_each_dependency_table`], mutable.
fn for_each_dependency_table_mut(
    manifest: &mut toml::map::Map<String, toml::Value>,
    f: &mut dyn FnMut(&mut toml::map::Map<String, toml::Value>),
) {
    for section in ["dependencies", "build-dependencies"] {
        if let Some(entries) = manifest.get_mut(section).and_then(toml::Value::as_table_mut) {
            f(entries);
        }
    }
    if let Some(targets) = manifest.get_mut("target").and_then(toml::Value::as_table_mut) {
        for target in targets.iter_mut().map(|(_, value)| value).filter_map(toml::Value::as_table_mut) {
            for section in ["dependencies", "build-dependencies"] {
                if let Some(entries) = target.get_mut(section).and_then(toml::Value::as_table_mut) {
                    f(entries);
                }
            }
        }
    }
}

/// One dependency of a crate manifest, as feature resolution needs it.
struct Dependency {
    /// Its name in the manifest (the key; features name it so).
    name: String,
    /// The crate directory of a local (path) dependency, canonical.
    path: Option<PathBuf>,
    optional: bool,
    default_features: bool,
    features: Vec<String>,
}

fn string_list(value: Option<&toml::Value>) -> Vec<String> {
    value
        .and_then(toml::Value::as_array)
        .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// Every dependency of `manifest` (at `dir`, canonical), with what it inherits
/// from the root's `[workspace.dependencies]`.
fn dependencies(
    manifest: &toml::map::Map<String, toml::Value>,
    dir: &Path,
    repo: &Path,
    workspace_dependencies: Option<&toml::map::Map<String, toml::Value>>,
) -> Result<Vec<Dependency>> {
    let mut found = Vec::new();
    let mut failure = None;
    for_each_dependency_table(manifest, &mut |_, entries| {
        for (name, value) in entries {
            let mut dependency = Dependency {
                name: name.clone(),
                path: None,
                optional: value.get("optional").and_then(toml::Value::as_bool).unwrap_or(false),
                default_features: value
                    .get("default-features")
                    .or_else(|| value.get("default_features"))
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
                features: string_list(value.get("features")),
            };
            if let Some(path) = value.get("path").and_then(toml::Value::as_str) {
                dependency.path = Some(dir.join(path));
            }
            if value.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
                let Some(inherited) = workspace_dependencies.and_then(|deps| deps.get(name)) else {
                    failure.get_or_insert_with(|| {
                        format!("{name} inherits a workspace dependency the root manifest does not declare")
                    });
                    continue;
                };
                if let Some(path) = inherited.get("path").and_then(toml::Value::as_str) {
                    dependency.path = Some(repo.join(path));
                }
                dependency.default_features = inherited
                    .get("default-features")
                    .or_else(|| inherited.get("default_features"))
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true);
                let mut features = string_list(inherited.get("features"));
                features.extend(dependency.features);
                dependency.features = features;
            }
            found.push(dependency);
        }
    });
    if let Some(failure) = failure {
        bail!("{failure} ('{}')", dir.join("Cargo.toml").display());
    }
    for dependency in &mut found {
        if let Some(path) = &dependency.path {
            dependency.path = Some(
                path.canonicalize()
                    .with_context(|| format!("resolving the local dependency '{}'", path.display()))?,
            );
        }
    }
    Ok(found)
}

/// What a crate is asked for: its default features (when any dependent, or
/// the selected crate itself, takes them) and named features.
#[derive(Default, Clone, PartialEq, Eq)]
struct Request {
    default: bool,
    features: BTreeSet<String>,
}

/// The features a [`Request`] turns on in one crate: every feature reached,
/// the optional dependencies they enable, and the features they ask of each
/// dependency (a weak `x?/f` only applies when `x` is on).
struct Activation {
    features: BTreeSet<String>,
    optional_on: BTreeSet<String>,
    dependency_features: Vec<(String, String, bool)>,
}

fn activate(
    manifest: &toml::map::Map<String, toml::Value>,
    dependencies: &[Dependency],
    request: &Request,
    dir: &Path,
) -> Result<Activation> {
    let table = manifest.get("features").and_then(toml::Value::as_table);
    let optional: BTreeSet<&str> = dependencies.iter().filter(|d| d.optional).map(|d| d.name.as_str()).collect();
    let mut activation = Activation {
        features: BTreeSet::new(),
        optional_on: BTreeSet::new(),
        dependency_features: Vec::new(),
    };
    let mut queue: Vec<String> = request.features.iter().cloned().collect();
    if request.default && table.is_some_and(|t| t.contains_key("default")) {
        queue.push("default".into());
    }
    while let Some(feature) = queue.pop() {
        if !activation.features.insert(feature.clone()) {
            continue;
        }
        if let Some(values) = table.and_then(|t| t.get(&feature)) {
            for value in string_list(Some(values)) {
                if let Some(name) = value.strip_prefix("dep:") {
                    activation.optional_on.insert(name.to_string());
                } else if let Some((name, wanted)) = value.split_once('/') {
                    let (name, weak) = match name.strip_suffix('?') {
                        Some(name) => (name, true),
                        None => (name, false),
                    };
                    if !weak && optional.contains(name) {
                        activation.optional_on.insert(name.to_string());
                    }
                    activation.dependency_features.push((name.to_string(), wanted.to_string(), weak));
                } else {
                    queue.push(value);
                }
            }
        } else if optional.contains(feature.as_str()) {
            // The implicit feature of an optional dependency.
            activation.optional_on.insert(feature);
        } else {
            bail!("feature '{feature}' is asked of '{}', which does not declare it", dir.display());
        }
    }
    Ok(activation)
}

/// The crate at `relative` and every local dependency its build takes, by
/// directory relative to `repo`, each with the manifest its prepared source
/// gets: dev-dependencies, tests, benches, examples and `package.metadata`
/// removed, and every optional dependency the build does not enable removed
/// with the features no build of the closure turns on. The build enables what
/// cargo would: the selected crate's default features (tool-crate.rhai and
/// reclogin.rhai pass no `--features`) and, transitively, what each dependent
/// asks of its dependencies. A feature a target's `required-features` names is
/// kept (emptied of what was removed), so the manifest stays valid.
fn resolve_closure(
    repo: &Path,
    relative: &Path,
    workspace_dependencies: Option<&toml::map::Map<String, toml::Value>>,
) -> Result<BTreeMap<PathBuf, toml::map::Map<String, toml::Value>>> {
    let mut manifests: BTreeMap<PathBuf, (toml::map::Map<String, toml::Value>, Vec<Dependency>)> = BTreeMap::new();
    let mut requests: BTreeMap<PathBuf, Request> = BTreeMap::new();
    requests.insert(relative.to_path_buf(), Request { default: true, features: BTreeSet::new() });
    loop {
        let mut changed = false;
        for dir in requests.keys().cloned().collect::<Vec<_>>() {
            if !manifests.contains_key(&dir) {
                let path = repo.join(&dir).join("Cargo.toml");
                let mut manifest: toml::Value = toml::from_str(
                    &fs::read_to_string(&path).with_context(|| format!("reading '{}'", path.display()))?,
                )
                .with_context(|| format!("parsing '{}'", path.display()))?;
                let table = manifest.as_table_mut().context("crate manifest must be a table")?;
                for key in ["dev-dependencies", "test", "bench", "example"] {
                    table.remove(key);
                }
                if let Some(package) = table.get_mut("package").and_then(toml::Value::as_table_mut) {
                    // Packaging data (RPM assets, docs.rs settings): cargo build never reads it.
                    package.remove("metadata");
                }
                if let Some(targets) = table.get_mut("target").and_then(toml::Value::as_table_mut) {
                    for target in targets.iter_mut().map(|(_, value)| value).filter_map(toml::Value::as_table_mut) {
                        target.remove("dev-dependencies");
                    }
                }
                let deps = dependencies(table, &repo.join(&dir), repo, workspace_dependencies)?;
                manifests.insert(dir.clone(), (table.clone(), deps));
            }
            let (manifest, deps) = &manifests[&dir];
            let activation = activate(manifest, deps, &requests[&dir], &repo.join(&dir))?;
            for dependency in deps {
                let Some(path) = &dependency.path else { continue };
                if dependency.optional && !activation.optional_on.contains(&dependency.name) {
                    continue;
                }
                let target = path
                    .strip_prefix(repo)
                    .with_context(|| format!("the local dependency '{}' is outside the repository", path.display()))?
                    .to_path_buf();
                let before = requests.get(&target).cloned();
                let request = requests.entry(target).or_default();
                request.default |= dependency.default_features;
                request.features.extend(dependency.features.iter().cloned());
                request.features.extend(
                    activation
                        .dependency_features
                        .iter()
                        .filter(|(name, _, _)| *name == dependency.name)
                        .map(|(_, wanted, _)| wanted.clone()),
                );
                changed |= before.as_ref() != Some(request);
            }
        }
        if !changed {
            break;
        }
    }
    let mut closure = BTreeMap::new();
    for (dir, request) in &requests {
        let (manifest, deps) = &manifests[dir];
        let activation = activate(manifest, deps, request, &repo.join(dir))?;
        let removed: BTreeSet<&str> = deps
            .iter()
            .filter(|d| d.optional && !activation.optional_on.contains(&d.name))
            .map(|d| d.name.as_str())
            .collect();
        let mut manifest = manifest.clone();
        for_each_dependency_table_mut(&mut manifest, &mut |entries| {
            entries.retain(|name, _| !removed.contains(name));
        });
        let mut required: BTreeSet<String> = BTreeSet::new();
        for kind in ["bin", "lib"] {
            match manifest.get(kind) {
                Some(toml::Value::Array(targets)) => {
                    for target in targets {
                        required.extend(string_list(target.get("required-features")));
                    }
                }
                Some(target @ toml::Value::Table(_)) => required.extend(string_list(target.get("required-features"))),
                _ => {}
            }
        }
        if let Some(features) = manifest.get_mut("features").and_then(toml::Value::as_table_mut) {
            let kept: BTreeSet<String> = features
                .keys()
                .filter(|name| activation.features.contains(*name) || required.contains(*name))
                .cloned()
                .collect();
            features.retain(|name, _| kept.contains(name));
            for values in features.iter_mut().map(|(_, value)| value).filter_map(toml::Value::as_array_mut) {
                values.retain(|value| {
                    let Some(value) = value.as_str() else { return true };
                    let named = value
                        .strip_prefix("dep:")
                        .or_else(|| value.split_once('/').map(|(name, _)| name.trim_end_matches('?')));
                    match named {
                        Some(dependency) => !removed.contains(dependency),
                        None => kept.contains(value) || deps.iter().any(|d| d.name == value && !removed.contains(value)),
                    }
                });
            }
        }
        closure.insert(dir.clone(), manifest);
    }
    Ok(closure)
}

/// These dependency directories are certificate test fixtures or an old
/// compiler-bootstrap script, not inputs to the shipped Rust libraries. Keep
/// them out of the production source import, whose key-material guard must
/// still reject unexpected files. Checksums of every retained file stay intact.
fn omit_vendor_development_files(vendor: &Path) -> Result<()> {
    for (package, directories) in [
        ("rustls-native-certs", &["tests", "integration-tests"][..]),
        ("rustls-pemfile", &["tests"][..]),
        ("untrusted", &["mk"][..]),
    ] {
        let root = vendor.join(package);
        if !root.is_dir() {
            continue;
        }
        let checksum_path = root.join(".cargo-checksum.json");
        let mut checksum: serde_json::Value = serde_json::from_slice(&fs::read(&checksum_path)?)?;
        let files = checksum.get_mut("files")
            .and_then(serde_json::Value::as_object_mut)
            .context("vendored crate checksum has no files map")?;
        for directory in directories {
            let path = root.join(directory);
            if path.is_dir() {
                fs::remove_dir_all(&path)?;
                let prefix = format!("{directory}/");
                files.retain(|name, _| !name.starts_with(&prefix));
            }
        }
        fs::write(checksum_path, serde_json::to_vec(&checksum)?)?;
    }
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

/// The crate directories (relative to the repository) `package`'s source
/// closure takes: its own and every local dependency's, as `copy_crate` walks
/// them.
pub(crate) fn tool_crate_dirs(repo: &Path, package: &str) -> Result<BTreeSet<PathBuf>> {
    let (_, relative, _) = tool(package)?;
    crate_closure_dirs(repo, Path::new(relative))
}

/// The crate directories (relative to the repository) of the crate at
/// `relative` and of every local dependency it takes, transitively, as
/// `copy_crate` walks them.
pub(crate) fn crate_closure_dirs(repo: &Path, relative: &Path) -> Result<BTreeSet<PathBuf>> {
    let repo = repo.canonicalize()?;
    let root_manifest: toml::Value = toml::from_str(&fs::read_to_string(repo.join("Cargo.toml"))?)?;
    let workspace_dependencies = root_manifest
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table);
    let mut dirs = BTreeSet::new();
    let mut todo = vec![repo.join(relative)];
    while let Some(dir) = todo.pop() {
        let original = dir.canonicalize()?;
        let relative = original
            .strip_prefix(&repo)
            .context("local tool dependency is outside the selected repository")?
            .to_owned();
        if !dirs.insert(relative) {
            continue;
        }
        let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(original.join("Cargo.toml"))?)?;
        let table = manifest.as_table_mut().context("crate manifest must be a table")?;
        table.remove("dev-dependencies");
        collect_local_dependencies(table, &original, &repo, workspace_dependencies, &mut todo)?;
    }
    Ok(dirs)
}

/// The files of the crate at `original` its compile reads, relative to it:
/// every file under `src/` that git knows of (tracked, or new and not
/// ignored), every file `src/` embeds at compile time, and `extra`. The
/// manifest is written separately.
///
/// What `src/` embeds is found by scanning its Rust files (and the Rust files
/// they pull in) for `include_str!`, `include_bytes!` and `include!` with a
/// string literal (relative to the file) or
/// `concat!(env!("CARGO_MANIFEST_DIR"), "...")`, and for `#[path = "..."]`
/// modules; any other argument fails, naming the macro. A bare
/// `env!("CARGO_MANIFEST_DIR")` is a path compiled in as text, not a file the
/// compile reads (the contract exporters' schema folders, used by
/// `cargo xtask gen` behind the `contracts` feature), so it adds nothing. A
/// build script would read files no scan can see: a crate with one fails here.
pub(crate) fn crate_files(original: &Path, extra: &[String]) -> Result<BTreeSet<PathBuf>> {
    if original.join("build.rs").exists() {
        bail!(
            "'{}' has a build script: list what it reads in crate_files (distro/builder/src/recipe/tools.rs)",
            original.display()
        );
    }
    let listing = Cmd::new("git")
        .arg("-C")
        .arg(original)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "src"])
        .error_msg(format!(
            "listing the files of crate '{}' (it must be in a git checkout)",
            original.display()
        ))
        .run()?;
    let mut files: BTreeSet<PathBuf> = listing
        .stdout
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(PathBuf::from)
        // Tracked but deleted in the working tree: the crate no longer has it.
        .filter(|name| fs::symlink_metadata(original.join(name)).is_ok())
        .collect();
    let mut todo: Vec<PathBuf> = files.iter().filter(|name| name.extension().is_some_and(|e| e == "rs")).cloned().collect();
    let mut scanned = BTreeSet::new();
    while let Some(file) = todo.pop() {
        if !scanned.insert(file.clone()) {
            continue;
        }
        let text = fs::read_to_string(original.join(&file))
            .with_context(|| format!("reading '{}'", original.join(&file).display()))?;
        for embedded in embedded_paths(&text, &file)
            .with_context(|| format!("scanning '{}' for files it embeds", original.join(&file).display()))?
        {
            let path = original.join(&embedded);
            let canonical = path
                .canonicalize()
                .with_context(|| format!("'{}' embeds '{}', which does not exist", file.display(), embedded.display()))?;
            let relative = canonical
                .strip_prefix(original.canonicalize()?)
                .with_context(|| {
                    format!("'{}' embeds '{}', which is outside its crate", file.display(), embedded.display())
                })?
                .to_path_buf();
            if relative.extension().is_some_and(|e| e == "rs") {
                todo.push(relative.clone());
            }
            files.insert(relative);
        }
    }
    for name in extra {
        let path = original.join(name);
        if !fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
            bail!("'{}' is not a file (named as a file the recipe reads)", path.display());
        }
        files.insert(PathBuf::from(name));
    }
    Ok(files)
}

/// The files `text` (the Rust file `file`, relative to its crate) embeds at
/// compile time, relative to the crate (see [`crate_files`]).
fn embedded_paths(text: &str, file: &Path) -> Result<Vec<PathBuf>> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let mut found = Vec::new();
    for line in text.lines() {
        let line = line.trim_start();
        if line.starts_with("//") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#[path") {
            let literal = rest
                .trim_start()
                .strip_prefix('=')
                .and_then(|rest| string_literal(rest.trim_start()))
                .with_context(|| format!("a #[path] attribute without a plain string: {line}"))?;
            found.push(dir.join(literal));
        }
        for mac in ["include_str!", "include_bytes!", "include!"] {
            let mut rest = line;
            while let Some(at) = rest.find(mac) {
                let before = rest[..at].chars().next_back();
                let after = rest[at + mac.len()..].trim_start();
                rest = &rest[at + mac.len()..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let Some(argument) = after.strip_prefix('(').map(str::trim_start) else { continue };
                if let Some(literal) = string_literal(argument) {
                    found.push(dir.join(literal));
                    continue;
                }
                let compact: String = argument.chars().filter(|c| !c.is_whitespace()).collect();
                let literal = compact
                    .strip_prefix("concat!(env!(\"CARGO_MANIFEST_DIR\"),")
                    .and_then(string_literal)
                    .with_context(|| {
                        format!(
                            "{mac} takes an argument the builder cannot resolve (a string literal or \
                             concat!(env!(\"CARGO_MANIFEST_DIR\"), \"...\") on one line): {line}"
                        )
                    })?;
                found.push(PathBuf::from(literal.trim_start_matches('/')));
            }
        }
    }
    Ok(found)
}

/// The plain string literal `text` starts with (no escapes), if it does.
fn string_literal(text: &str) -> Option<&str> {
    let body = text.strip_prefix('"')?;
    let end = body.find('"')?;
    let literal = &body[..end];
    (!literal.contains('\\')).then_some(literal)
}

/// Copies the files [`crate_files`] selects from the crate at `original` into
/// `output`, links kept as links.
fn copy_crate_files(original: &Path, output: &Path, extra: &[String]) -> Result<()> {
    fs::create_dir_all(output)?;
    for name in crate_files(original, extra)? {
        let source = original.join(&name);
        let metadata = fs::symlink_metadata(&source).with_context(|| format!("reading '{}'", source.display()))?;
        let target = output.join(&name);
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

/// Compile every binary of `package` once, in one cargo build through
/// `os-recipes/tool-crate.rhai`, and return the directory holding one program
/// per binary. The result is shared by every init system and distro; the work
/// directory is keyed by package alone. Two distro builds may run at once, so
/// the whole invocation holds an exclusive lock: the second waits, then finds
/// the first's published programs ready and reuses them. The programs come from
/// the prebuilt cache or a download when one matches this exact source (`crate::prebuilt`),
/// and are compiled only otherwise.
pub(crate) fn compile_crate(repo: &Path, package: &str) -> Result<PathBuf> {
    Ok(compile_crate_with_origin(repo, package)?.0)
}

/// [`compile_crate`], also saying whether the programs came from the local cache, a
/// download or this compile.
pub(crate) fn compile_crate_with_origin(repo: &Path, package: &str) -> Result<(PathBuf, crate::prebuilt::Origin)> {
    use crate::prebuilt::{self, Family};
    let (_, relative, binaries) = tool(package)?;
    let crates = repo.join(".artifacts/work/recipe-tools/crates");
    fs::create_dir_all(&crates)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(crates.join(format!("{package}.lock")))?;
    use std::os::fd::AsRawFd;
    anyhow::ensure!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } == 0,
        "cannot lock the {package} crate build: {}",
        std::io::Error::last_os_error()
    );
    let source = prepare_tool_source(repo, package)?;
    let recipe = repo
        .join("distro/builder/os-recipes")
        .join("tool-crate.rhai");
    // The local cache, then a download, then this compile (see `crate::prebuilt`).
    let obtained = prebuilt::obtain(
        repo,
        Family::Tools,
        package,
        || tool_key(repo, package, source.path()),
        binaries,
        || {
            prebuilt::source_commits(
                repo,
                crate_closure_dirs(repo, Path::new(relative)).unwrap_or_default(),
            )
        },
        || {
            let result = super::Invocation::new(repo, "install", &recipe, crates.join(package))
                .define("BINS", &binaries.join(","))
                .input("source", source.path())
                .discard_superseded()
                .run()?;
            let bins = result.directory("bins")?;
            Ok(binaries
                .iter()
                .map(|name| (name.to_string(), bins.join(name)))
                .collect())
        },
    )?;
    // Every program of the crate is in one directory, as the recipe published it
    // or as the cache holds it.
    let bins = obtained
        .path(binaries[0])?
        .parent()
        .context("a program has a directory")?
        .to_path_buf();
    drop(lock);
    Ok((bins, obtained.origin))
}

/// The key of `package`'s prebuilt programs: its prepared source closure, the compile
/// recipe, target, compiler image and Recipe's declared behaviour (`crate::prebuilt`).
fn tool_key(repo: &Path, package: &str, source: &Path) -> Result<crate::prebuilt::Key> {
    let (_, _, binaries) = tool(package)?;
    let mut sorted = binaries.to_vec();
    sorted.sort_unstable();
    let recipe = repo.join("distro/builder/os-recipes/tool-crate.rhai");
    Ok(crate::prebuilt::Key::new(&[
        ("family", "tools".into()),
        ("package", package.into()),
        ("binaries", sorted.join(",")),
        ("target", crate::prebuilt::TOOLS_TARGET.into()),
        ("source_closure", crate::prebuilt::dir_digest(source)?),
        ("tool_crate_recipe", crate::prebuilt::file_digest(&recipe)?),
        ("compiler_image", super::compiler_image_reference(repo)?),
        ("behaviour", crate::prebuilt::behaviour()),
    ]))
}

/// `distro-builder prebuilt key tools <package>`: prepares the source closure and
/// computes the key.
pub fn prebuilt_key(repo: &Path, package: &str) -> Result<crate::prebuilt::Key> {
    let source = prepare_tool_source(repo, package)?;
    tool_key(repo, package, source.path())
}

/// The binaries `package` builds, the files of its prebuilt.
pub fn tool_binaries(package: &str) -> Result<Vec<String>> {
    Ok(tool(package)?.2.iter().map(|name| name.to_string()).collect())
}

/// Where the repository keeps the release public keys (`*.pub`) every image
/// carries at `/usr/share/levitate/release-keys/` (with recupdate's tree).
pub(crate) const RELEASE_KEYS_DIR: &str = "distro/keys/release";

/// Compile `package`'s `binary` (its default when `None`) through its recipe,
/// as a static musl program on every distro, and return the published package tree,
/// which holds it at `usr/bin/<binary>`. recupdate's tree also depends on the
/// init system (its service files): use [`build_system_tool_tree`] for it.
pub(crate) fn build_tool_tree(repo: &Path, package: &str, binary: Option<&str>) -> Result<PathBuf> {
    build_system_tool_tree(repo, package, binary, None)
}

/// [`build_tool_tree`] for a system whose init is `init`: recupdate's tree
/// takes the init's service files from `tools/recupdate/dist` and the release
/// public keys from [`RELEASE_KEYS_DIR`] (a build without one says so).
pub(crate) fn build_system_tool_tree(
    repo: &Path,
    package: &str,
    binary: Option<&str>,
    init: Option<distro_profile::InitSystem>,
) -> Result<PathBuf> {
    let (_, _, binaries) = tool(package)?;
    let binary = binary.unwrap_or(binaries[0]);
    if !binaries.contains(&binary) {
        bail!("package {package} does not build binary {binary}");
    }
    let source = prepare_tool_source(repo, package)?;
    let recipe = repo
        .join("distro/builder/os-recipes")
        .join("install-tools.rhai");
    let init_name = if package == "recupdate" {
        Some(
            init.with_context(|| "recupdate's tree is made for one init system; name it")?
                .as_str(),
        )
    } else {
        None
    };
    let work = repo.join(".artifacts/work/recipe-tools").join(match init_name {
        Some(init) => format!("{binary}-{init}"),
        None => binary.to_string(),
    });
    let install_path = format!("usr/bin/{binary}");
    let mut invocation = super::Invocation::new(repo, "install", &recipe, &work)
        .define("BIN", binary)
        .define("INSTALL_PATH", &install_path)
        .input("source", source.path())
        .input("program", &compile_crate(repo, package)?.join(binary));
    let keys = tempfile::Builder::new().prefix("release-keys-").tempdir()?;
    if let Some(init) = init_name {
        let found = copy_release_keys(repo, keys.path())?;
        if found == 0 {
            crate::tprintln!(
                "[release-keys] no key in {RELEASE_KEYS_DIR}/*.pub: this image will refuse every release \
                 (make one with `cargo xtask release keygen`)"
            );
        }
        invocation = invocation
            .define("INIT", init)
            .input("dist", &repo.join("tools/recupdate/dist"))
            .input("keys", keys.path());
    }
    // The package manager ships the same pinned runtime the builder uses.
    // Explicit inputs give the recipe no ambient access to the host image.
    let reference_file = tempfile::NamedTempFile::new()?;
    if package == "levitate-recipe" {
        let (reference, image) = super::runtime_image(repo)?;
        fs::write(reference_file.path(), format!("{reference}\n"))?;
        invocation = invocation
            .input("catalog", &repo.join("recipes"))
            .input("launcher", super::ginux_binary(repo)?)
            .input("runtime_image", &image)
            .input("runtime_reference", reference_file.path());
    }
    let tree = invocation.discard_superseded().run()?.directory("rootfs")?;
    drop(keys);
    Ok(tree)
}

/// Copies every `*.pub` of [`RELEASE_KEYS_DIR`] into `into` (nothing else of
/// that folder: its README stays out of the image) and returns how many.
fn copy_release_keys(repo: &Path, into: &Path) -> Result<usize> {
    let dir = repo.join(RELEASE_KEYS_DIR);
    let mut found = 0;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).with_context(|| format!("reading {}", dir.display())),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".pub") && entry.file_type()?.is_file() {
            names.push(name);
        }
    }
    names.sort();
    for name in names {
        let text = fs::read_to_string(dir.join(&name)).with_context(|| format!("reading {RELEASE_KEYS_DIR}/{name}"))?;
        let key = text.trim();
        if key.len() != 44 || !key.ends_with('=') || text.trim_end_matches('\n').contains('\n') {
            bail!("{RELEASE_KEYS_DIR}/{name} is not one base64 Ed25519 public key line (cargo xtask release keygen prints it)");
        }
        fs::write(into.join(&name), format!("{key}\n"))?;
        found += 1;
    }
    Ok(found)
}
