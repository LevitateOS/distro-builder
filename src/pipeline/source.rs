use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::paths::{normalize_distro_id, resolve_repo_path};
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::recipe::rocky_stage01::{
    materialize_rootfs, materialize_rootfs_from_recipe, RockyStage01RecipeSpec,
    Stage01RootfsRecipeSpec,
};

#[derive(Debug, Clone)]
pub(crate) enum S01RootfsSourcePolicy {
    RecipeRocky {
        recipe_script: PathBuf,
        iso_name: String,
        sha256: String,
        sha256_url: String,
        torrent_url: String,
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
    iso_name: Option<String>,
    sha256: Option<String>,
    sha256_url: Option<String>,
    torrent_url: Option<String>,
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
            let iso_name = source.iso_name.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.iso_name is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let sha256 = source.sha256.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.sha256 is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let sha256_url = source.sha256_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.sha256_url is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let torrent_url = source.torrent_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.torrent_url is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            Ok(Some(S01RootfsSourcePolicy::RecipeRocky {
                recipe_script,
                iso_name: iso_name.trim().to_string(),
                sha256: sha256.trim().to_string(),
                sha256_url: sha256_url.trim().to_string(),
                torrent_url: torrent_url.trim().to_string(),
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
        Some(S01RootfsSourcePolicy::RecipeRocky {
            recipe_script,
            iso_name,
            sha256,
            sha256_url,
            torrent_url,
        }) => {
            let build_dir = rootfs_provider_work_dir(repo_root, distro_id)?.join("rocky");
            let work_downloads_dir = downloads_work_dir(repo_root, distro_id)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 Rocky source provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs(
                repo_root,
                &build_dir,
                &RockyStage01RecipeSpec {
                    recipe_script: recipe_script.clone(),
                    iso_name: iso_name.clone(),
                    sha256: sha256.clone(),
                    sha256_url: sha256_url.clone(),
                    torrent_url: torrent_url.clone(),
                    preseed_iso_path: work_downloads_dir.join(iso_name),
                    trust_dir: work_downloads_dir,
                },
            )
            .with_context(|| {
                format!(
                    "materializing Stage 01 Rocky source rootfs for '{}'",
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
            let build_dir = rootfs_provider_work_dir(repo_root, distro_id)?.join("custom");
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 custom source provider directory '{}'",
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
                    "materializing Stage 01 custom source rootfs for '{}'",
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
            iso_name: None,
            sha256: None,
            sha256_url: None,
            torrent_url: None,
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
