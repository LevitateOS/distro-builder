use anyhow::{Context, Result};
use distro_contract::STAGE_01_REQUIRED_LIVE_SERVICES_BASE;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::pipeline::io::{
    create_unique_output_dir, extract_erofs_rootfs, resolve_parent_stage_rootfs_image,
};
use crate::pipeline::live_tools::add_required_tools;
use crate::pipeline::overlay::{
    create_live_overlay, ensure_required_service_wiring, ensure_systemd_locale_completeness,
    S01OverlayPolicy,
};
use crate::pipeline::scripts::install_stage_test_scripts;
use crate::stages::s01_boot_inputs::load_s01_boot_input_spec;

const STAGE02_ARTIFACT_TAG: &str = "s02";

#[derive(Debug, Clone)]
pub struct S02LiveToolsInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S02LiveToolsInputSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    overlay: S01OverlayPolicy,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02LiveToolsToml {
    stage_02: S02StageToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02StageToml {
    live_tools: S02LiveToolsInputsToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02LiveToolsInputsToml {
    os_name: String,
}

pub fn load_s02_live_tools_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S02LiveToolsInputSpec> {
    let config_path = variant_dir.join("02LiveTools.toml");
    let config_bytes = fs::read_to_string(&config_path)
        .with_context(|| format!("reading Stage 02 config '{}'", config_path.display()))?;
    let parsed: S02LiveToolsToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing Stage 02 config '{}'", config_path.display()))?;

    let s01_spec =
        load_s01_boot_input_spec(repo_root, variant_dir, distro_id).with_context(|| {
            format!(
                "loading Stage 01 overlay baseline while preparing Stage 02 for '{}'",
                distro_id
            )
        })?;

    Ok(S02LiveToolsInputSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: parsed.stage_02.live_tools.os_name,
        rootfs_source_dir: PathBuf::from("s02-rootfs-source"),
        overlay: s01_spec.overlay,
    })
}

pub fn prepare_s02_live_tools_inputs(
    spec: &S02LiveToolsInputSpec,
    output_dir: &Path,
) -> Result<S02LiveToolsInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 02 live tools input output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    let parent_rootfs = resolve_parent_stage_rootfs_image(
        output_dir,
        "s01-boot",
        "Stage 01",
        "s01-filesystem.erofs",
        true,
    )?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent stage rootfs from '{}'",
            parent_rootfs.display()
        )
    })?;

    add_required_tools(&spec.repo_root, &rootfs_source_dir, &spec.distro_id)
        .with_context(|| format!("adding Stage 02 required tools for '{}'", spec.distro_id))?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into Stage 02 rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 02 locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let live_overlay_dir = create_live_overlay(
        output_dir,
        &spec.distro_id,
        &spec.os_name,
        "S02 Live Tools",
        STAGE02_ARTIFACT_TAG,
        &spec.overlay,
    )?;

    let required_services = STAGE_01_REQUIRED_LIVE_SERVICES_BASE
        .iter()
        .map(|svc| (*svc).to_string())
        .collect::<Vec<_>>();
    ensure_required_service_wiring(&live_overlay_dir, &spec.overlay, &required_services)
        .with_context(|| {
            format!(
                "ensuring Stage 01 service wiring in 02LiveTools overlay for '{}'",
                spec.distro_id
            )
        })?;

    Ok(S02LiveToolsInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}
