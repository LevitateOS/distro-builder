//! Orchestrate Recipe's live overlay producer; no live policy writes the rootfs.
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use distro_contract::profile::InitSystem;
use serde::Serialize;

/// The `config` input of `recipes/live-overlay.rhai`: its only schema.
#[derive(Serialize)]
struct LiveOverlayConfig<'a> {
    os_name: &'a str,
    issue: String,
    init: InitSystem,
    consoles: &'a [String],
    services: &'a [String],
    launcher: String,
    shell_ready_marker: &'a str,
    role_file: &'a str,
    role: &'a str,
}

pub(crate) fn create_live_overlay(
    repo: &Path,
    output_dir: &Path,
    rootfs: &Path,
    distro_id: &str,
    os_name: &str,
    overlay_label: &str,
    dir_name: &str,
    init: InitSystem,
    consoles: &[String],
    services: &[String],
) -> Result<PathBuf> {
    let input = tempfile::tempdir_in(output_dir)?;
    let config = input.path().join("config.json");
    fs::write(
        &config,
        serde_json::to_vec(&LiveOverlayConfig {
            os_name,
            issue: format!(
                "\n{os_name} {overlay_label} - \\l\n\n{}\n\n",
                crate::install_ux::LIVE_LOGIN_HINT
            ),
            init,
            consoles,
            services,
            launcher: format!("/{}", crate::install_ux::LAUNCHER),
            shell_ready_marker: recqemu::serial::protocol::SHELL_READY_MARKER,
            role_file: recinit::live::PAYLOAD_ROLE_FILE,
            role: recinit::live::ROLE_OVERLAY,
        })?,
    )?;
    let serial_profile = input.path().join("serial-profile.sh");
    fs::write(&serial_profile, recqemu::serial::protocol::guest_profile())?;
    let assets = repo.join("distro/builder/guest");
    let shadow = rootfs.join("etc/shadow");
    let recipe = repo.join("distro/builder/recipes/live-overlay.rhai");
    let work = crate::recipe::recipe_work_dir(repo, distro_id)?.join(dir_name);
    let bin = crate::recipe::find_recipe(repo)?;
    let ctx = crate::recipe::run_recipe_phase_json_with_inputs(
        &bin.path,
        "install",
        &recipe,
        &work,
        &[],
        &[
            ("config", &config),
            ("serial_profile", &serial_profile),
            ("assets", &assets),
            ("shadow", &shadow),
        ],
        &[],
        None,
        recipe.parent(),
    )
    .context("preparing the live overlay with Recipe")?;
    let published = crate::recipe::outputs::directory(&ctx, "overlay")?;
    let destination = crate::pipeline::io::create_empty_overlay_dir(output_dir, dir_name)?;
    levitate_recipe::target::merge_tree(&published, &destination)?;
    Ok(destination)
}
