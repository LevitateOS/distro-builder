//! The result of a finished Recipe run (specification 8.6) and its named
//! outputs. Publication remains coordinator-owned.
//!
//! Recipe writes every value: an output path used exactly as reported, and
//! one that is empty or has surrounding whitespace is rejected.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The `schema_version` of a finished-run result this builder reads.
const SCHEMA_VERSION: u64 = 1;

/// The result a recipe run printed and wrote to `--json-output`.
#[derive(Debug)]
pub struct RecipeResult {
    json: Value,
}

fn exact(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && text.trim() == *text)
}

impl RecipeResult {
    /// A result from Recipe's `--json-output`: a JSON object of schema 1 with
    /// an `outputs` object (empty for a run that published nothing).
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let json: Value =
            serde_json::from_slice(bytes).context("parsing the recipe JSON result")?;
        if !json.is_object() {
            bail!("the recipe result must be a JSON object");
        }
        if let Some(failure) = json.get("failure") {
            bail!("the recipe reported a failure: {failure}");
        }
        match json.get("schema_version").and_then(Value::as_u64) {
            Some(SCHEMA_VERSION) => {}
            other => bail!(
                "the recipe result has schema_version {other:?}; this builder reads {SCHEMA_VERSION}"
            ),
        }
        if !json.get("outputs").is_some_and(Value::is_object) {
            bail!("the recipe result must have an outputs object");
        }
        Ok(Self { json })
    }

    /// The whole result, as Recipe reported it.
    pub(crate) fn json(&self) -> &Value {
        &self.json
    }

    fn outputs(&self) -> Result<&serde_json::Map<String, Value>> {
        self.json
            .get("outputs")
            .and_then(Value::as_object)
            .context("the recipe result must have an outputs object with named products")
    }

    fn declared_path(&self, name: &str) -> Result<PathBuf> {
        let path = self.outputs()?.get(name).and_then(exact).with_context(|| {
            format!("the recipe must report outputs.{name} as a nonempty path string")
        })?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            bail!(
                "outputs.{name} must be an absolute published path: '{}'",
                path.display()
            );
        }
        Ok(path)
    }

    pub fn file(&self, name: &str) -> Result<PathBuf> {
        let path = self.declared_path(name)?;
        let metadata = std::fs::metadata(&path).with_context(|| {
            format!("outputs.{name} file is unavailable at '{}'", path.display())
        })?;
        if !metadata.is_file() {
            bail!("outputs.{name} must identify a file: '{}'", path.display());
        }
        Ok(path)
    }

    pub fn directory(&self, name: &str) -> Result<PathBuf> {
        let path = self.declared_path(name)?;
        let metadata = std::fs::metadata(&path).with_context(|| {
            format!(
                "outputs.{name} directory is unavailable at '{}'",
                path.display()
            )
        })?;
        if !metadata.is_dir() {
            bail!(
                "outputs.{name} must identify a directory: '{}'",
                path.display()
            );
        }
        Ok(path)
    }
}

/// Atomically select one already-published directory. A pre-existing physical
/// tree is retained beside the selection rather than destroyed or overwritten.
pub(super) fn select_directory(published: &Path, destination: &Path) -> Result<()> {
    use std::fs;
    use std::os::unix::fs::symlink;
    if !published.is_absolute() || !published.is_dir() {
        bail!("selected publication must be an existing absolute directory");
    }
    let parent = destination
        .parent()
        .context("selection destination has no parent")?;
    fs::create_dir_all(parent)?;
    let next = tempfile::Builder::new()
        .prefix(".selection-next-")
        .tempfile_in(parent)?;
    fs::remove_file(next.path())?;
    symlink(published, next.path())?;
    let mut previous = None;
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if !metadata.file_type().is_symlink() {
            let saved = tempfile::Builder::new()
                .prefix("previous-")
                .tempdir_in(parent)?
                .keep();
            fs::remove_dir(&saved)?;
            fs::rename(destination, &saved)?;
            previous = Some(saved);
        }
    }
    if let Err(error) = fs::rename(next.path(), destination) {
        if let Some(previous) = previous {
            fs::rename(previous, destination)?;
        }
        return Err(error).context("selecting published directory");
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}
