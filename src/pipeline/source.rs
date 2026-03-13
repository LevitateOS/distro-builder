use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::paths::{normalize_distro_id, resolve_repo_path};
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::recipe::rocky_stage01::{
    materialize_rootfs_from_recipe, Stage01RootfsRecipeSpec,
};

#[derive(Debug, Clone)]
pub(crate) enum S01RootfsSourcePolicy {
    RecipeRocky {
        recipe_script: PathBuf,
    },
    RecipeCustom {
        recipe_script: PathBuf,
        defines: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct S01RootfsSourceToml {
    kind: String,
    recipe_script: String,
    #[serde(rename = "iso_name")]
    _legacy_iso_name: Option<String>,
    #[serde(rename = "sha256")]
    _legacy_sha256: Option<String>,
    #[serde(rename = "sha256_url")]
    _legacy_sha256_url: Option<String>,
    #[serde(rename = "torrent_url")]
    _legacy_torrent_url: Option<String>,
    defines: Option<BTreeMap<String, String>>,
}

pub(crate) fn parse_rootfs_source_policy(
    repo_root: &Path,
    config_path: &Path,
    source: Option<S01RootfsSourceToml>,
) -> Result<Option<S01RootfsSourcePolicy>> {
    let Some(source) = source else {
        return Ok(None);
    };

    match source.kind.trim().to_ascii_lowercase().as_str() {
        "recipe_rocky" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            Ok(Some(S01RootfsSourcePolicy::RecipeRocky {
                recipe_script,
            }))
        }
        "recipe_custom" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            Ok(Some(S01RootfsSourcePolicy::RecipeCustom {
                recipe_script,
                defines: source.defines.unwrap_or_default(),
            }))
        }
        other => bail!(
            "invalid Stage 01 config '{}': unsupported rootfs_source.kind '{}'",
            config_path.display(),
            other
        ),
    }
}

pub(crate) fn materialize_source_rootfs(
    repo_root: &Path,
    distro_id: &str,
    source_policy: &Option<S01RootfsSourcePolicy>,
) -> Result<PathBuf> {
    match source_policy {
        Some(S01RootfsSourcePolicy::RecipeRocky { recipe_script, .. }) => {
            let build_dir = rootfs_provider_recipe_work_dir(repo_root, distro_id, recipe_script)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 recipe source provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs_from_recipe(
                repo_root,
                &build_dir,
                &Stage01RootfsRecipeSpec {
                    recipe_script: recipe_script.clone(),
                    defines: BTreeMap::new(),
                },
            )
            .with_context(|| {
                format!(
                    "materializing Stage 01 recipe source rootfs for '{}'",
                    distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        Some(S01RootfsSourcePolicy::RecipeCustom {
            recipe_script,
            defines,
        }) => {
            let build_dir = rootfs_provider_recipe_work_dir(repo_root, distro_id, recipe_script)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 recipe source provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs_from_recipe(
                repo_root,
                &build_dir,
                &Stage01RootfsRecipeSpec {
                    recipe_script: recipe_script.clone(),
                    defines: defines.clone(),
                },
            )
            .with_context(|| {
                format!(
                    "materializing Stage 01 recipe source rootfs for '{}'",
                    distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        None => bail!(
            "Stage 01 producer plan requires copy-based rootfs source, but no rootfs_source policy is configured for '{}'.",
            distro_id
        ),
    }
}

pub(crate) fn cleanup_legacy_provider_dir(output_dir: &Path) -> Result<()> {
    let legacy = output_dir.join("stage01-rootfs-provider");
    if legacy.is_dir() {
        fs::remove_dir_all(&legacy)
            .with_context(|| format!("removing legacy provider dir '{}'", legacy.display()))?;
    }
    Ok(())
}

fn rootfs_provider_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "rootfs provider work directory")?;
    let provider_dir = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("stage01-rootfs-provider");
    fs::create_dir_all(&provider_dir).with_context(|| {
        format!(
            "creating Stage 01 rootfs provider work directory '{}'",
            provider_dir.display()
        )
    })?;
    Ok(provider_dir)
}

fn rootfs_provider_recipe_work_dir(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
) -> Result<PathBuf> {
    let recipe_dir_name = recipe_script
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("recipe");
    Ok(rootfs_provider_work_dir(repo_root, distro_id)?.join(recipe_dir_name))
}

#[cfg(test)]
fn downloads_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "work downloads directory")?;
    let downloads = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads");
    fs::create_dir_all(&downloads).with_context(|| {
        format!(
            "creating Stage 01 work downloads directory '{}'",
            downloads.display()
        )
    })?;
    Ok(downloads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rootfs_source_policy_accepts_custom_recipe_for_any_distro() {
        let source = S01RootfsSourceToml {
            kind: "recipe_custom".to_string(),
            recipe_script: "distro-builder/recipes/custom-stage01-rootfs.rhai".to_string(),
            _legacy_iso_name: None,
            _legacy_sha256: None,
            _legacy_sha256_url: None,
            _legacy_torrent_url: None,
            defines: Some(BTreeMap::from([(
                "CUSTOM_ROOTFS_DIR".to_string(),
                "/tmp/rootfs".to_string(),
            )])),
        };
        let policy = parse_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/acorn/01Boot.toml"),
            Some(source),
        )
        .expect("parsing custom rootfs_source policy must succeed");

        assert!(matches!(
            policy,
            Some(S01RootfsSourcePolicy::RecipeCustom { .. })
        ));
    }

    #[test]
    fn rootfs_source_policy_accepts_rocky_recipe_without_metadata_fields() {
        let source = S01RootfsSourceToml {
            kind: "recipe_rocky".to_string(),
            recipe_script: "distro-builder/recipes/rocky-stage01-rootfs.rhai".to_string(),
            _legacy_iso_name: None,
            _legacy_sha256: None,
            _legacy_sha256_url: None,
            _legacy_torrent_url: None,
            defines: None,
        };
        let policy = parse_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/levitate/01Boot.toml"),
            Some(source),
        )
        .expect("parsing recipe_rocky without metadata must succeed");

        assert!(matches!(
            policy,
            Some(S01RootfsSourcePolicy::RecipeRocky { .. })
        ));
    }

    #[test]
    fn downloads_work_dir_normalizes_aliases() {
        let unique = format!(
            "levitateos-s01-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("create temp repo root");

        let downloads =
            downloads_work_dir(&repo_root, "leviso").expect("resolve alias downloads dir");
        assert!(
            downloads.ends_with(".artifacts/work/levitate/downloads"),
            "expected normalized levitate work downloads path, got {}",
            downloads.display()
        );
    }

    #[test]
    fn downloads_work_dir_rejects_unknown_distro() {
        let unique = format!(
            "levitateos-s01-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("create temp repo root");

        let result = downloads_work_dir(&repo_root, "unknown");
        assert!(result.is_err(), "unknown distro should fail");
    }
}
