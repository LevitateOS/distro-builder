//! Shared Linux kernel recipe wrapper.

use super::{find_recipe, recipe_work_dir, run_recipe_phase_json_with_inputs};
use crate::pipeline::paths::normalize_distro_id;
use anyhow::{Context, Result};
use distro_contract::{load_variant_contract_bundle_for_distro_from, require_valid_contract};
use std::path::{Path, PathBuf};

/// Paths produced by the linux.rhai recipe after execution.
#[derive(Debug, Clone)]
pub struct LinuxPaths {
    /// Reported source tree, when retained by the producer.
    pub source: Option<PathBuf>,
    /// Path to vmlinuz in staging.
    pub vmlinuz: PathBuf,
    /// Kernel version string.
    pub version: String,
}

impl LinuxPaths {
    /// Whether the returned image remains present; recipe checks determine readiness.
    pub fn is_installed(&self) -> bool {
        self.vmlinuz.is_file()
    }
}

/// Run the source kernel producer with canonical variant configuration and work paths.
pub fn linux_for_distro(repo_root: &Path, distro_id: &str) -> Result<LinuxPaths> {
    let distro_id = normalize_distro_id(distro_id, "Linux adapter")?;
    let bundle = load_variant_contract_bundle_for_distro_from(repo_root, distro_id)
        .with_context(|| format!("loading kernel configuration for '{distro_id}'"))?;
    require_valid_contract(&bundle.contract)
        .with_context(|| format!("validating kernel contract for '{distro_id}'"))?;
    let monorepo_dir = &bundle.repo_root;
    let work_dir = recipe_work_dir(monorepo_dir, distro_id)?;
    // Single SSOT kernel recipe for all distros (parity). Distro-specific overrides
    // (e.g. <distro>/deps/linux.rhai) are intentionally ignored.
    let shared_recipe = monorepo_dir.join("distro-builder/recipes/linux.rhai");
    if !shared_recipe.exists() {
        anyhow::bail!(
            "Linux recipe not found at expected shared path:\n  - {}",
            shared_recipe.display()
        );
    }
    let recipe_path = shared_recipe;

    let kconfig_path = bundle
        .paths
        .build_host_declared_path(&bundle.contract.build.kernel.kconfig_path);
    let fragments = kconfig_path
        .parent()
        .context("kconfig has no parent")?
        .join("kconfig.d");
    let current = crate::run_history::kernel_output_dir(monorepo_dir, distro_id);
    let mut inputs: Vec<(&str, &Path)> = vec![("kconfig", &kconfig_path)];
    if fragments.is_dir() {
        inputs.push(("kconfig_fragments", &fragments));
    }
    if current.is_dir() {
        inputs.push(("target", &current));
    }
    let defines = [("KERNEL_TARGET", distro_id)];

    // Find and run recipe, parse JSON output
    let recipes_dir = monorepo_dir.join("distro-builder/recipes");
    let recipe_bin = find_recipe(&monorepo_dir)?;
    let ctx = run_recipe_phase_json_with_inputs(
        &recipe_bin.path,
        "install",
        &recipe_path,
        &work_dir,
        &defines,
        &inputs,
        &[],
        None,
        Some(&recipes_dir),
    )?;

    select_kernel_bundle(&ctx, &current)?;
    let vmlinuz = super::outputs::file(&ctx, "kernel")?;
    // Modules are a required companion product, even though callers currently
    // consume only the image path from this return type.
    super::outputs::directory(&ctx, "modules")?;
    let source = super::outputs::optional_directory(&ctx, "source")?;
    let version = super::outputs::metadata_string(&ctx, "kernel_version")?.to_owned();
    Ok(LinuxPaths {
        source,
        vmlinuz,
        version,
    })
}

/// Select the explicit immutable bundle returned by Recipe for development
/// consumers. Existing physical artifact trees are retained, never overwritten.
#[cfg(unix)]
pub fn select_kernel_bundle(ctx: &serde_json::Value, destination: &Path) -> Result<()> {
    let bundle = super::outputs::directory(ctx, "bundle")?;
    super::outputs::file(ctx, "kernel")?;
    super::outputs::directory(ctx, "modules")?;
    super::outputs::select_directory(&bundle, destination)
}
