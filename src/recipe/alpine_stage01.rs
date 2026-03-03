use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{find_recipe, run_recipe_phase_json_with_defines};
use crate::pipeline::paths::normalize_distro_id;

pub const ALPINE_STAGE01_RECIPE_FILENAME: &str = "alpine-stage01-rootfs.rhai";

const KEY_RELEASE_BRANCH: &str = "ALPINE_RELEASE_BRANCH";
const KEY_EXTENDED_VERSION: &str = "ALPINE_EXTENDED_VERSION";
const KEY_EXTENDED_SHA256: &str = "ALPINE_EXTENDED_SHA256";
const KEY_APK_TOOLS_VERSION: &str = "ALPINE_APK_TOOLS_VERSION";
const KEY_APK_TOOLS_SHA256: &str = "ALPINE_APK_TOOLS_SHA256";
const KEY_PRESEED_ISO: &str = "ALPINE_PRESEED_ISO";
const KEY_PRESEED_APK_TOOLS: &str = "ALPINE_PRESEED_APK_TOOLS";
const KEY_TRUST_DIR: &str = "ALPINE_TRUST_DIR";

#[derive(Debug, Clone)]
pub struct AlpineStage01PreseedSpec {
    pub release_branch: String,
    pub extended_version: String,
    pub extended_sha256: String,
    pub apk_tools_version: String,
    pub apk_tools_sha256: String,
    pub preseed_iso_path: PathBuf,
    pub preseed_apk_tools_path: PathBuf,
    pub trust_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AlpineStage01PreseedOutput {
    pub iso_path: PathBuf,
    pub apk_tools_path: PathBuf,
}

pub fn is_alpine_stage01_recipe(recipe_script: &Path) -> bool {
    recipe_script
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name == ALPINE_STAGE01_RECIPE_FILENAME)
        .unwrap_or(false)
}

pub fn preseed_spec_from_defines(
    repo_root: &Path,
    distro_id: &str,
    defines: &BTreeMap<String, String>,
) -> Result<AlpineStage01PreseedSpec> {
    let release_branch = required_define(defines, KEY_RELEASE_BRANCH, distro_id)?;
    let extended_version = required_define(defines, KEY_EXTENDED_VERSION, distro_id)?;
    let extended_sha256 = required_define(defines, KEY_EXTENDED_SHA256, distro_id)?;
    let apk_tools_version = required_define(defines, KEY_APK_TOOLS_VERSION, distro_id)?;
    let apk_tools_sha256 = required_define(defines, KEY_APK_TOOLS_SHA256, distro_id)?;

    let normalized = normalize_distro_id(distro_id, "Stage 01 Alpine preseed")?;
    let trust_dir = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads");
    let iso_name = format!("alpine-extended-{}-x86_64.iso", extended_version);
    let apk_tools_name = format!("apk-tools-static-{}.apk", apk_tools_version);

    Ok(AlpineStage01PreseedSpec {
        release_branch,
        extended_version,
        extended_sha256,
        apk_tools_version,
        apk_tools_sha256,
        preseed_iso_path: trust_dir.join(iso_name),
        preseed_apk_tools_path: trust_dir.join(apk_tools_name),
        trust_dir,
    })
}

pub fn augment_defines_with_preseed_paths(
    defines: &BTreeMap<String, String>,
    spec: &AlpineStage01PreseedSpec,
) -> BTreeMap<String, String> {
    let mut merged = defines.clone();
    merged.insert(
        KEY_PRESEED_ISO.to_string(),
        spec.preseed_iso_path.display().to_string(),
    );
    merged.insert(
        KEY_PRESEED_APK_TOOLS.to_string(),
        spec.preseed_apk_tools_path.display().to_string(),
    );
    merged.insert(
        KEY_TRUST_DIR.to_string(),
        spec.trust_dir.display().to_string(),
    );
    merged
}

pub fn preseed_alpine_stage01_assets(
    repo_root: &Path,
    spec: &AlpineStage01PreseedSpec,
    refresh: bool,
) -> Result<AlpineStage01PreseedOutput> {
    let recipe_path = repo_root.join("distro-builder/recipes/alpine-preseed-stage01-assets.rhai");
    if !recipe_path.is_file() {
        bail!(
            "Alpine preseed recipe script not found: '{}'",
            recipe_path.display()
        );
    }

    std::fs::create_dir_all(&spec.trust_dir).with_context(|| {
        format!(
            "creating Alpine preseed trust directory '{}'",
            spec.trust_dir.display()
        )
    })?;

    if let Some(parent) = spec.preseed_iso_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "creating Alpine preseed ISO parent directory '{}'",
                parent.display()
            )
        })?;
    }
    if let Some(parent) = spec.preseed_apk_tools_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "creating Alpine preseed apk-tools parent directory '{}'",
                parent.display()
            )
        })?;
    }

    let recipe_bin =
        find_recipe(repo_root).context("resolving recipe binary for Alpine Stage 01 preseed")?;
    let recipes_path = recipe_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Alpine preseed recipe has no parent directory: '{}'",
            recipe_path.display()
        )
    })?;

    let mut defines = BTreeMap::from([
        (KEY_RELEASE_BRANCH.to_string(), spec.release_branch.clone()),
        (
            KEY_EXTENDED_VERSION.to_string(),
            spec.extended_version.clone(),
        ),
        (
            KEY_EXTENDED_SHA256.to_string(),
            spec.extended_sha256.clone(),
        ),
        (
            KEY_APK_TOOLS_VERSION.to_string(),
            spec.apk_tools_version.clone(),
        ),
        (
            KEY_APK_TOOLS_SHA256.to_string(),
            spec.apk_tools_sha256.clone(),
        ),
        (
            KEY_PRESEED_ISO.to_string(),
            spec.preseed_iso_path.display().to_string(),
        ),
        (
            KEY_PRESEED_APK_TOOLS.to_string(),
            spec.preseed_apk_tools_path.display().to_string(),
        ),
        (
            KEY_TRUST_DIR.to_string(),
            spec.trust_dir.display().to_string(),
        ),
    ]);
    if refresh {
        defines.insert("ALPINE_FORCE_REFRESH".to_string(), "1".to_string());
    }

    let mut define_refs: Vec<(&str, &str)> = Vec::with_capacity(defines.len());
    for (key, value) in &defines {
        define_refs.push((key.as_str(), value.as_str()));
    }

    let ctx = run_recipe_phase_json_with_defines(
        &recipe_bin.path,
        "install",
        &recipe_path,
        &spec.trust_dir,
        &define_refs,
        Some(recipes_path),
    )
    .with_context(|| {
        format!(
            "preseeding Alpine Stage 01 assets via '{}'",
            recipe_path.display()
        )
    })?;

    let iso_path = ctx["iso_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| spec.preseed_iso_path.clone());
    let apk_tools_path = ctx["apk_tools_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| spec.preseed_apk_tools_path.clone());

    if !iso_path.is_file() {
        bail!(
            "Alpine preseed recipe did not produce ISO at '{}'",
            iso_path.display()
        );
    }
    if !apk_tools_path.is_file() {
        bail!(
            "Alpine preseed recipe did not produce apk-tools package at '{}'",
            apk_tools_path.display()
        );
    }

    Ok(AlpineStage01PreseedOutput {
        iso_path,
        apk_tools_path,
    })
}

fn required_define(
    defines: &BTreeMap<String, String>,
    key: &str,
    distro_id: &str,
) -> Result<String> {
    let value = defines.get(key).map(|raw| raw.trim()).unwrap_or_default();
    if value.is_empty() {
        bail!(
            "Stage 01 config for '{}' is missing required define '{}' for Alpine preseed",
            distro_id,
            key
        );
    }
    Ok(value.to_string())
}
