use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::config::load_boot_config;
use crate::pipeline::io::{
    create_empty_overlay_dir, create_unique_output_dir, extract_erofs_rootfs,
    resolve_parent_stage_rootfs_image,
};
use crate::pipeline::overlay::{
    create_live_overlay, ensure_openrc_shell, ensure_required_service_wiring,
    ensure_systemd_default_target, ensure_systemd_locale_completeness, ensure_systemd_sshd_dirs,
    S01OverlayPolicy,
};
#[cfg(test)]
use crate::pipeline::plan::ensure_non_legacy_rootfs_source;
use crate::pipeline::plan::{
    apply_producer_plan, boot_baseline_producers, build_baseline_producers, ProducerPlan,
    RootfsProducer,
};
use crate::pipeline::scripts::install_stage_test_scripts;
use crate::pipeline::source::{
    cleanup_legacy_provider_dir, materialize_source_rootfs, S01RootfsSourcePolicy,
};

const STAGE00_ARTIFACT_TAG: &str = "s00";
const STAGE01_ARTIFACT_TAG: &str = "s01";

#[derive(Debug, Clone)]
pub struct S00BuildInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S01BootInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S00BuildInputSpec {
    pub distro_id: String,
    pub os_name: String,
    pub os_id: String,
    pub rootfs_source_dir: PathBuf,
    plan: ProducerPlan,
}

#[derive(Debug, Clone)]
pub struct S01BootInputSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    add_plan: ProducerPlan,
    required_services: Vec<String>,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub overlay: S01OverlayPolicy,
}

pub fn load_s00_build_input_spec(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    _output_root: &Path,
) -> Result<S00BuildInputSpec> {
    Ok(S00BuildInputSpec {
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

pub fn prepare_s00_build_inputs(
    spec: &S00BuildInputSpec,
    output_dir: &Path,
) -> Result<S00BuildInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 00 build input output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    apply_producer_plan(&spec.plan, &rootfs_source_dir)
        .with_context(|| format!("materializing Stage 00 rootfs for '{}'", spec.distro_id))?;

    let live_overlay_dir = create_empty_overlay_dir(output_dir, STAGE00_ARTIFACT_TAG)
        .with_context(|| format!("creating empty overlay for {}", spec.distro_id))?;

    Ok(S00BuildInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn load_s01_boot_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01BootInputSpec> {
    let loaded = load_boot_config(repo_root, variant_dir, distro_id)?;

    let mut add_producers = boot_baseline_producers(match &loaded.overlay {
        S01OverlayPolicy::Systemd { .. } => "systemd",
        S01OverlayPolicy::OpenRc { .. } => "openrc",
    });
    if loaded.rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(S01BootInputSpec {
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

pub fn prepare_s01_boot_inputs(
    spec: &S01BootInputSpec,
    output_dir: &Path,
) -> Result<S01BootInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 01 boot input output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    cleanup_legacy_provider_dir(output_dir).with_context(|| {
        format!(
            "cleaning legacy Stage 01 provider directory under '{}'",
            output_dir.display()
        )
    })?;
    let parent_rootfs = resolve_parent_stage_rootfs_image(
        output_dir,
        "s00-build",
        "Stage 00",
        "s00-filesystem.erofs",
        false,
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent stage rootfs from '{}'",
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
            "applying Stage 01 additive producers for '{}'",
            spec.distro_id
        )
    })?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into Stage 01 rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&rootfs_source_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC Stage 01 serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_default_target(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 default target for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_sshd_dirs(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 sshd directories for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        "S01 Boot",
        STAGE01_ARTIFACT_TAG,
        &spec.overlay,
    )?;

    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_shell(&live_overlay_dir, &spec.os_name, inittab).with_context(|| {
            format!(
                "ensuring OpenRC Stage 01 serial shell for '{}'",
                spec.distro_id
            )
        })?;
    }
    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &spec.required_services)
        .with_context(|| {
            format!(
                "ensuring Stage 01 required service wiring for '{}'",
                spec.distro_id
            )
        })?;

    Ok(S01BootInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage00_baseline_contains_os_release_files() {
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
