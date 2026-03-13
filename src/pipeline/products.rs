use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::config::load_boot_config;
use crate::pipeline::io::{
    create_empty_overlay_dir, create_unique_output_dir, extract_erofs_rootfs,
    resolve_parent_stage_rootfs_image,
};
use crate::pipeline::live_tools::{add_required_tools, Stage02InstallExperience};
use crate::pipeline::overlay::{
    create_live_overlay, ensure_openrc_shell, ensure_required_service_wiring,
    ensure_systemd_default_target, ensure_systemd_locale_completeness, ensure_systemd_sshd_dirs,
    S01OverlayPolicy,
};
use crate::pipeline::plan::{
    apply_producer_plan, boot_baseline_producers, build_baseline_producers, ProducerPlan,
    RootfsProducer,
};
#[cfg(test)]
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::pipeline::scripts::install_stage_test_scripts;
use crate::pipeline::source::{
    cleanup_legacy_provider_dir, materialize_source_rootfs, S01RootfsSourcePolicy,
};
use crate::recipe::alpine_stage01::is_alpine_stage01_recipe;

const COMPAT_STAGE00_ARTIFACT_TAG: &str = "s00";
const COMPAT_STAGE01_ARTIFACT_TAG: &str = "s01";
const COMPAT_STAGE02_ARTIFACT_TAG: &str = "s02";

#[derive(Debug, Clone)]
pub struct BaseRootfsProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LiveBootProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct LiveToolsProduct {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct BaseRootfsProductSpec {
    pub distro_id: String,
    pub os_name: String,
    pub os_id: String,
    pub rootfs_source_dir: PathBuf,
    plan: ProducerPlan,
}

#[derive(Debug, Clone)]
pub struct LiveBootProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    add_plan: ProducerPlan,
    required_services: Vec<String>,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub overlay: S01OverlayPolicy,
}

impl LiveBootProductSpec {
    pub fn required_services(&self) -> &[String] {
        &self.required_services
    }

    pub fn uses_rpm_dvd_rootfs_source(&self) -> bool {
        matches!(
            self.rootfs_source_policy,
            Some(S01RootfsSourcePolicy::RecipeRpmDvd { .. })
        )
    }

    pub fn rpm_dvd_preseed_recipe_script(&self) -> Option<&Path> {
        let Some(S01RootfsSourcePolicy::RecipeRpmDvd {
            preseed_recipe_script,
            ..
        }) = &self.rootfs_source_policy
        else {
            return None;
        };
        Some(preseed_recipe_script.as_path())
    }

    pub fn uses_alpine_stage01_rootfs_source(&self) -> bool {
        let Some(S01RootfsSourcePolicy::RecipeCustom { recipe_script, .. }) =
            &self.rootfs_source_policy
        else {
            return false;
        };
        is_alpine_stage01_recipe(recipe_script)
    }
}

#[derive(Debug, Clone)]
pub struct LiveToolsProductSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    install_experience: Stage02InstallExperience,
    pub rootfs_source_dir: PathBuf,
    overlay: S01OverlayPolicy,
    required_services: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolsToml {
    stage_02: LiveToolsStageToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolsStageToml {
    live_tools: LiveToolsInputsToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolsInputsToml {
    os_name: String,
    install_experience: Stage02InstallExperience,
}

pub fn load_base_rootfs_product_spec(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    _output_root: &Path,
) -> Result<BaseRootfsProductSpec> {
    Ok(BaseRootfsProductSpec {
        distro_id: distro_id.to_string(),
        os_name: os_name.to_string(),
        os_id: os_id.to_string(),
        rootfs_source_dir: PathBuf::from("s00-rootfs-source"),
        plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: build_baseline_producers(distro_id, os_name, os_id),
        },
    })
}

pub fn prepare_base_rootfs_product(
    spec: &BaseRootfsProductSpec,
    output_dir: &Path,
) -> Result<BaseRootfsProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating base rootfs product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    apply_producer_plan(&spec.plan, &rootfs_source_dir)
        .with_context(|| format!("materializing base rootfs for '{}'", spec.distro_id))?;

    let live_overlay_dir = create_empty_overlay_dir(output_dir, COMPAT_STAGE00_ARTIFACT_TAG)
        .with_context(|| format!("creating empty overlay for {}", spec.distro_id))?;

    Ok(BaseRootfsProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn load_live_boot_product_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<LiveBootProductSpec> {
    let loaded = load_boot_config(repo_root, variant_dir, distro_id)?;

    let mut add_producers = boot_baseline_producers(match &loaded.overlay {
        S01OverlayPolicy::Systemd { .. } => "systemd",
        S01OverlayPolicy::OpenRc { .. } => "openrc",
    });
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(LiveBootProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: loaded.os_name,
        rootfs_source_dir: PathBuf::from("s01-rootfs-source"),
        add_plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: add_producers,
        },
        required_services: loaded.required_services,
        rootfs_source_policy: loaded.rootfs_source_policy,
        overlay: loaded.overlay,
    })
}

pub fn prepare_live_boot_product(
    spec: &LiveBootProductSpec,
    output_dir: &Path,
) -> Result<LiveBootProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating live boot product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    cleanup_legacy_provider_dir(output_dir).with_context(|| {
        format!(
            "cleaning legacy live boot provider directory under '{}'",
            output_dir.display()
        )
    })?;
    let parent_rootfs = resolve_parent_stage_rootfs_image(
        output_dir,
        "s00-build",
        "Stage 00",
        "s00-filesystem.erofs",
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for live boot product from '{}'",
            parent_rootfs.display()
        )
    })?;

    let mut add_plan = spec.add_plan.clone();
    if add_plan.producers.iter().any(|producer| {
        matches!(
            producer,
            RootfsProducer::CopyTree { .. }
                | RootfsProducer::CopySymlink { .. }
                | RootfsProducer::CopyFile { .. }
        )
    }) {
        let source_rootfs_dir = materialize_source_rootfs(
            &spec.repo_root,
            &spec.distro_id,
            &spec.rootfs_source_policy,
        )?;
        add_plan.source_rootfs_dir = Some(source_rootfs_dir);
    }

    apply_producer_plan(&add_plan, &rootfs_source_dir).with_context(|| {
        format!(
            "applying live boot product additive producers for '{}'",
            spec.distro_id
        )
    })?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into live boot rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&rootfs_source_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC live boot serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_default_target(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot default target for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_sshd_dirs(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot sshd directories for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live boot locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        "S01 Boot",
        COMPAT_STAGE01_ARTIFACT_TAG,
        &spec.overlay,
    )?;

    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&live_overlay_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC live overlay serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &spec.required_services)
        .with_context(|| {
            format!(
                "ensuring live boot required service wiring for '{}'",
                spec.distro_id
            )
        })?;

    Ok(LiveBootProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn materialize_live_boot_source_rootfs(spec: &LiveBootProductSpec) -> Result<PathBuf> {
    materialize_source_rootfs(&spec.repo_root, &spec.distro_id, &spec.rootfs_source_policy)
}

pub fn load_live_tools_product_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<LiveToolsProductSpec> {
    let config_path = variant_dir.join("02LiveTools.toml");
    let config_bytes = fs::read_to_string(&config_path)
        .with_context(|| format!("reading Stage 02 config '{}'", config_path.display()))?;
    let parsed: LiveToolsToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing Stage 02 config '{}'", config_path.display()))?;

    let live_boot_spec = load_live_boot_product_spec(repo_root, variant_dir, distro_id)
        .with_context(|| {
            format!(
                "loading live boot baseline while preparing live tools for '{}'",
                distro_id
            )
        })?;

    Ok(LiveToolsProductSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: parsed.stage_02.live_tools.os_name,
        install_experience: parsed.stage_02.live_tools.install_experience,
        rootfs_source_dir: PathBuf::from("s02-rootfs-source"),
        overlay: live_boot_spec.overlay.clone(),
        required_services: live_boot_spec.required_services().to_vec(),
    })
}

pub fn prepare_live_tools_product(
    spec: &LiveToolsProductSpec,
    output_dir: &Path,
) -> Result<LiveToolsProduct> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating live tools product output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    let parent_rootfs = resolve_parent_stage_rootfs_image(
        output_dir,
        "s01-boot",
        "Stage 01",
        "s01-filesystem.erofs",
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent rootfs for live tools product from '{}'",
            parent_rootfs.display()
        )
    })?;

    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into live tools rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd live tools locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        "S02 Live Tools",
        COMPAT_STAGE02_ARTIFACT_TAG,
        &spec.overlay,
    )?;

    add_required_tools(
        &spec.repo_root,
        &rootfs_source_dir,
        &live_overlay_dir,
        &spec.distro_id,
        spec.install_experience,
    )
    .with_context(|| format!("adding required live tools for '{}'", spec.distro_id))?;

    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &spec.required_services)
        .with_context(|| {
            format!(
                "ensuring live tools required service wiring for '{}'",
                spec.distro_id
            )
        })?;

    Ok(LiveToolsProduct {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_rootfs_baseline_contains_os_release_files() {
        let producers = build_baseline_producers("levitate", "LevitateOS", "levitateos");
        let paths: Vec<PathBuf> = producers
            .iter()
            .filter_map(|p| match p {
                RootfsProducer::WriteText { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(paths.contains(&PathBuf::from("etc/os-release")));
        assert!(paths.contains(&PathBuf::from("usr/lib/stage-manifest.json")));
    }

    #[test]
    fn legacy_rootfs_source_is_rejected() {
        let mut legacy = PathBuf::from("/data/vince/LevitateOS");
        for component in ["leviso", "downloads", "rootfs"] {
            legacy.push(component);
        }
        let result = ensure_non_legacy_rootfs_source(&legacy);
        assert!(result.is_err(), "legacy rootfs path must fail policy");
    }

    #[test]
    fn missing_rootfs_source_filters_copy_producers() {
        let mut producers = boot_baseline_producers("openrc");
        producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));

        assert!(!producers.is_empty());
        assert!(producers
            .iter()
            .all(|producer| matches!(producer, RootfsProducer::WriteText { .. })));
    }

    #[test]
    fn stage_scoped_rootfs_source_is_allowed() {
        let stage_scoped = Path::new(
            "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/s01-rootfs-source-12345-12345",
        );
        let result = ensure_non_legacy_rootfs_source(stage_scoped);
        assert!(
            result.is_ok(),
            "stage-scoped rootfs path should be accepted"
        );
    }
}
