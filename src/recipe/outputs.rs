//! Consumer contracts for named recipe products. Publication remains coordinator-owned.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::PathBuf;

fn declared_path(ctx: &Value, name: &str) -> Result<PathBuf> {
    let outputs = ctx
        .get("outputs")
        .and_then(Value::as_object)
        .context("recipe must report a ctx.outputs object with named products")?;
    let path = outputs
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
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

pub(super) fn file(ctx: &Value, name: &str) -> Result<PathBuf> {
    let path = declared_path(ctx, name)?;
    let metadata = std::fs::metadata(&path).with_context(|| {
        format!(
            "ctx.outputs.{name} file is unavailable at '{}'",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        bail!(
            "ctx.outputs.{name} must identify a file: '{}'",
            path.display()
        );
    }
    Ok(path)
}

pub(super) fn directory(ctx: &Value, name: &str) -> Result<PathBuf> {
    let path = declared_path(ctx, name)?;
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

pub(super) fn optional_directory(ctx: &Value, name: &str) -> Result<Option<PathBuf>> {
    let outputs = ctx
        .get("outputs")
        .and_then(Value::as_object)
        .context("recipe must report a ctx.outputs object with named products")?;
    if !outputs.contains_key(name) {
        return Ok(None);
    }
    directory(ctx, name).map(Some)
}

pub(super) fn metadata_string<'a>(ctx: &'a Value, name: &str) -> Result<&'a str> {
    ctx.get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .with_context(|| format!("recipe must report ctx.{name} as a nonempty metadata string"))
}
