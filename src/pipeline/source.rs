use anyhow::{bail, Context, Result};
use distro_contract::{ConformanceContract, RootfsSourceContract, RootfsSourceKind};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
use serde::Deserialize;

use crate::pipeline::paths::{normalize_distro_id, resolve_repo_path};
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::recipe::rootfs_source::{materialize_rootfs_from_recipe, RootfsSourceRecipeSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RootfsSourcePolicy {
    RecipeRpmDvd {
        recipe_script: PathBuf,
        preseed_recipe_script: PathBuf,
    },
    RecipeCustom {
        recipe_script: PathBuf,
        defines: BTreeMap<String, String>,
    },
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RootfsSourceToml {
    kind: String,
    recipe_script: String,
    preseed_recipe_script: Option<String>,
    defines: Option<BTreeMap<String, String>>,
}

#[cfg(test)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring3SourcesToml {
    #[allow(dead_code)]
    schema_version: u32,
    ring3_sources: Ring3SourcesSectionToml,
}

#[cfg(test)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring3SourcesSectionToml {
    rootfs_source: Option<RootfsSourceToml>,
}

#[cfg(test)]
pub(crate) fn load_rootfs_source_policy(
    repo_root: &Path,
    variant_dir: &Path,
) -> Result<Option<RootfsSourcePolicy>> {
    let ring3_config_path = variant_dir.join("ring3-sources.toml");
    if !ring3_config_path.is_file() {
        bail!(
            "missing canonical Ring 3 source owner for '{}': expected '{}'",
            variant_dir.display(),
            ring3_config_path.display()
        );
    }

    let config_bytes = fs::read_to_string(&ring3_config_path).with_context(|| {
        format!(
            "reading Ring 3 source config '{}'",
            ring3_config_path.display()
        )
    })?;
    let parsed: Ring3SourcesToml = toml::from_str(&config_bytes).with_context(|| {
        format!(
            "parsing Ring 3 source config '{}'",
            ring3_config_path.display()
        )
    })?;
    let source = parsed.ring3_sources.rootfs_source.clone().ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical Ring 3 source owner '[ring3_sources.rootfs_source]' in '{}'",
            ring3_config_path.display()
        )
    })?;

    parse_rootfs_source_policy(repo_root, &ring3_config_path, Some(source))
}

#[cfg(test)]
pub(crate) fn parse_rootfs_source_policy(
    repo_root: &Path,
    config_path: &Path,
    source: Option<RootfsSourceToml>,
) -> Result<Option<RootfsSourcePolicy>> {
    let Some(source) = source else {
        return Ok(None);
    };

    match source.kind.trim().to_ascii_lowercase().as_str() {
        "recipe_rpm_dvd" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            let preseed_recipe_script = match source.preseed_recipe_script {
                Some(script) => resolve_repo_path(repo_root, script.trim()),
                None => bail!(
                    "invalid rootfs source config '{}': rootfs_source.preseed_recipe_script is required for kind='recipe_rpm_dvd'",
                    config_path.display()
                ),
            };
            Ok(Some(RootfsSourcePolicy::RecipeRpmDvd {
                recipe_script,
                preseed_recipe_script,
            }))
        }
        "recipe_custom" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            Ok(Some(RootfsSourcePolicy::RecipeCustom {
                recipe_script,
                defines: source.defines.unwrap_or_default(),
            }))
        }
        other => bail!(
            "invalid rootfs source config '{}': unsupported rootfs_source.kind '{}'",
            config_path.display(),
            other
        ),
    }
}

pub(crate) fn rootfs_source_policy_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<Option<RootfsSourcePolicy>> {
    Ok(Some(rootfs_source_policy_from_source_contract(
        repo_root,
        &contract.sources.rootfs_source,
    )?))
}

fn rootfs_source_policy_from_source_contract(
    repo_root: &Path,
    source: &RootfsSourceContract,
) -> Result<RootfsSourcePolicy> {
    match source.kind {
        RootfsSourceKind::RecipeRpmDvd => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            let Some(preseed_recipe_script) = source.preseed_recipe_script.as_deref() else {
                bail!(
                    "invalid canonical Ring 3 contract: sources.rootfs_source.preseed_recipe_script is required for kind='recipe_rpm_dvd'"
                );
            };
            Ok(RootfsSourcePolicy::RecipeRpmDvd {
                recipe_script,
                preseed_recipe_script: resolve_repo_path(repo_root, preseed_recipe_script.trim()),
            })
        }
        RootfsSourceKind::RecipeCustom => Ok(RootfsSourcePolicy::RecipeCustom {
            recipe_script: resolve_repo_path(repo_root, source.recipe_script.trim()),
            defines: source.defines.clone(),
        }),
    }
}

pub(crate) fn materialize_source_rootfs(
    repo_root: &Path,
    distro_id: &str,
    source_policy: &Option<RootfsSourcePolicy>,
) -> Result<PathBuf> {
    match source_policy {
        Some(RootfsSourcePolicy::RecipeRpmDvd { recipe_script, .. }) => {
            let build_dir =
                rootfs_source_provider_recipe_work_dir(repo_root, distro_id, recipe_script)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating rootfs source recipe provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs_from_recipe(
                repo_root,
                &build_dir,
                &RootfsSourceRecipeSpec {
                    recipe_script: recipe_script.clone(),
                    defines: BTreeMap::new(),
                },
            )
            .with_context(|| {
                format!(
                    "materializing rootfs source recipe output for '{}'",
                    distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        Some(RootfsSourcePolicy::RecipeCustom {
            recipe_script,
            defines,
        }) => {
            let build_dir =
                rootfs_source_provider_recipe_work_dir(repo_root, distro_id, recipe_script)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating rootfs source recipe provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs_from_recipe(
                repo_root,
                &build_dir,
                &RootfsSourceRecipeSpec {
                    recipe_script: recipe_script.clone(),
                    defines: defines.clone(),
                },
            )
            .with_context(|| {
                format!(
                    "materializing rootfs source recipe output for '{}'",
                    distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        None => bail!(
            "live-boot product preparation requires a copy-based rootfs source, but no canonical rootfs_source policy is configured for '{}'.",
            distro_id
        ),
    }
}

pub(crate) fn cleanup_legacy_provider_dir(output_dir: &Path) -> Result<()> {
    let entries = fs::read_dir(output_dir)
        .with_context(|| format!("reading output directory '{}'", output_dir.display()))?;
    for entry in entries {
        let entry = entry.with_context(|| {
            format!(
                "reading legacy provider directory entry under '{}'",
                output_dir.display()
            )
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with("-rootfs-provider") {
            continue;
        }
        fs::remove_dir_all(&path)
            .with_context(|| format!("removing legacy provider dir '{}'", path.display()))?;
    }
    Ok(())
}

fn rootfs_source_provider_work_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "rootfs source provider work directory")?;
    let provider_dir = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("rootfs-source-provider");
    fs::create_dir_all(&provider_dir).with_context(|| {
        format!(
            "creating rootfs source provider work directory '{}'",
            provider_dir.display()
        )
    })?;
    Ok(provider_dir)
}

fn rootfs_source_provider_recipe_work_dir(
    repo_root: &Path,
    distro_id: &str,
    recipe_script: &Path,
) -> Result<PathBuf> {
    let recipe_dir_name = recipe_script
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("recipe");
    Ok(rootfs_source_provider_work_dir(repo_root, distro_id)?.join(recipe_dir_name))
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
            "creating rootfs source work downloads directory '{}'",
            downloads.display()
        )
    })?;
    Ok(downloads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_repo_root(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "distro-builder-source-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn rootfs_source_policy_accepts_custom_recipe_for_any_distro() {
        let source = RootfsSourceToml {
            kind: "recipe_custom".to_string(),
            recipe_script: "distro-builder/recipes/custom-source-rootfs.rhai".to_string(),
            preseed_recipe_script: None,
            defines: Some(BTreeMap::from([(
                "CUSTOM_ROOTFS_DIR".to_string(),
                "/tmp/rootfs".to_string(),
            )])),
        };
        let policy = parse_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/acorn/ring3-sources.toml"),
            Some(source),
        )
        .expect("parsing custom rootfs_source policy must succeed");

        assert!(matches!(
            policy,
            Some(RootfsSourcePolicy::RecipeCustom { .. })
        ));
    }

    #[test]
    fn rootfs_source_policy_accepts_neutral_rpm_dvd_kind() {
        let source = RootfsSourceToml {
            kind: "recipe_rpm_dvd".to_string(),
            recipe_script: "distro-builder/recipes/fedora-dvd-source-rootfs.rhai".to_string(),
            preseed_recipe_script: Some(
                "distro-builder/recipes/fedora-preseed-iso.rhai".to_string(),
            ),
            defines: None,
        };
        let policy = parse_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/levitate/ring3-sources.toml"),
            Some(source),
        )
        .expect("parsing recipe_rpm_dvd must succeed");

        assert!(matches!(
            policy,
            Some(RootfsSourcePolicy::RecipeRpmDvd { .. })
        ));
    }

    #[test]
    fn rootfs_source_policy_rejects_legacy_recipe_rocky_kind() {
        let source = RootfsSourceToml {
            kind: "recipe_rocky".to_string(),
            recipe_script: "distro-builder/recipes/rocky-dvd-source-rootfs.rhai".to_string(),
            preseed_recipe_script: None,
            defines: None,
        };
        let err = parse_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/levitate/ring3-sources.toml"),
            Some(source),
        )
        .expect_err("legacy recipe_rocky kind must fail");

        assert!(
            err.to_string()
                .contains("unsupported rootfs_source.kind 'recipe_rocky'"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn ring3_rootfs_source_policy_loads_from_canonical_owner() {
        let repo_root = temp_repo_root("ring3-canonical");
        let variant_dir = repo_root.join("distro-variants/levitate");
        write_file(
            &variant_dir.join("ring3-sources.toml"),
            r#"schema_version = 6

[ring3_sources.rootfs_source]
kind = "recipe_rpm_dvd"
recipe_script = "distro-builder/recipes/fedora-dvd-source-rootfs.rhai"
preseed_recipe_script = "distro-builder/recipes/fedora-preseed-iso.rhai"
"#,
        );

        let loaded = load_rootfs_source_policy(&repo_root, &variant_dir)
            .expect("load ring3 rootfs source policy");
        assert!(matches!(
            loaded,
            Some(RootfsSourcePolicy::RecipeRpmDvd { .. })
        ));

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn ring3_rootfs_source_policy_requires_canonical_owner() {
        let repo_root = temp_repo_root("ring3-missing");
        let variant_dir = repo_root.join("distro-variants/levitate");
        let err = load_rootfs_source_policy(&repo_root, &variant_dir)
            .expect_err("missing ring3 rootfs source should fail");
        assert!(
            err.to_string()
                .contains("missing canonical Ring 3 source owner"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
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
