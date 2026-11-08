//! A recipe run's ctx and its named outputs. Publication remains coordinator-owned.
//!
//! Recipe writes every value: an output path or metadata string is used exactly as
//! reported, and one that is empty or has surrounding whitespace is rejected.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The ctx a recipe run reported.
#[derive(Debug)]
pub struct RecipeResult {
    ctx: Value,
}

fn exact(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|text| !text.is_empty() && text.trim() == *text)
}

impl RecipeResult {
    /// A ctx from Recipe's `--json-output`; it must be a JSON object.
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let ctx: Value =
            serde_json::from_slice(bytes).context("parsing the recipe JSON result")?;
        if !ctx.is_object() {
            bail!("the recipe result must be a ctx JSON object");
        }
        Ok(Self { ctx })
    }

    fn outputs(&self) -> Result<&serde_json::Map<String, Value>> {
        self.ctx
            .get("outputs")
            .and_then(Value::as_object)
            .context("recipe must report a ctx.outputs object with named products")
    }

    fn declared_path(&self, name: &str) -> Result<PathBuf> {
        let path = self
            .outputs()?
            .get(name)
            .and_then(exact)
            .with_context(|| {
                format!("recipe must report ctx.outputs.{name} as a nonempty path string")
            })?;
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            bail!(
                "ctx.outputs.{name} must be an absolute published path: '{}'",
                path.display()
            );
        }
        Ok(path)
    }

    pub fn file(&self, name: &str) -> Result<PathBuf> {
        let path = self.declared_path(name)?;
        let metadata = std::fs::metadata(&path).with_context(|| {
            format!("ctx.outputs.{name} file is unavailable at '{}'", path.display())
        })?;
        if !metadata.is_file() {
            bail!("ctx.outputs.{name} must identify a file: '{}'", path.display());
        }
        Ok(path)
    }

    pub fn directory(&self, name: &str) -> Result<PathBuf> {
        let path = self.declared_path(name)?;
        let metadata = std::fs::metadata(&path).with_context(|| {
            format!(
                "ctx.outputs.{name} directory is unavailable at '{}'",
                path.display()
            )
        })?;
        if !metadata.is_dir() {
            bail!(
                "ctx.outputs.{name} must identify a directory: '{}'",
                path.display()
            );
        }
        Ok(path)
    }

    pub fn optional_directory(&self, name: &str) -> Result<Option<PathBuf>> {
        if !self.outputs()?.contains_key(name) {
            return Ok(None);
        }
        self.directory(name).map(Some)
    }

    pub fn metadata_string(&self, name: &str) -> Result<&str> {
        self.ctx.get(name).and_then(exact).with_context(|| {
            format!("recipe must report ctx.{name} as a nonempty metadata string")
        })
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
