//! Shared Linux kernel recipe wrapper.

use super::{find_recipe, run_recipe_json_with_defines};
use anyhow::Result;
use distro_spec::shared::KernelSource;
use std::path::{Path, PathBuf};

/// Paths produced by the linux.rhai recipe after execution.
#[derive(Debug, Clone)]
pub struct LinuxPaths {
    /// Path to the kernel source tree.
    pub source: PathBuf,
    /// Path to vmlinuz in staging.
    pub vmlinuz: PathBuf,
    /// Kernel version string.
    pub version: String,
}

impl LinuxPaths {
    /// Check if kernel is built and installed.
    pub fn is_installed(&self) -> bool {
        self.vmlinuz.exists()
    }
}

/// Run the linux.rhai recipe and return the output paths.
///
/// # Arguments
/// * `base_dir` - distro crate root (e.g., `/path/to/AcornOS`)
/// * `kernel_source` - Kernel spec from distro-spec (version, sha256, localversion)
pub fn linux(base_dir: &Path, kernel_source: &KernelSource) -> Result<LinuxPaths> {
    let monorepo_dir = base_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base_dir.to_path_buf());

    let downloads_dir = base_dir.join("downloads");

    // Look for distro-specific recipe first, then shared recipe in distro-builder
    let distro_recipe = base_dir.join("deps/linux.rhai");
    let shared_recipe = monorepo_dir.join("distro-builder/recipes/linux.rhai");
    let recipe_path = if distro_recipe.exists() {
        distro_recipe
    } else if shared_recipe.exists() {
        shared_recipe
    } else {
        anyhow::bail!(
            "Linux recipe not found.\n\
             Searched:\n  - {}\n  - {}",
            distro_recipe.display(),
            shared_recipe.display()
        );
    };

    // Inject kernel spec from distro-spec SSOT
    let defines: Vec<(&str, &str)> = vec![
        ("KERNEL_VERSION", kernel_source.version),
        ("KERNEL_SHA256", kernel_source.sha256),
        ("KERNEL_LOCALVERSION", kernel_source.localversion),
    ];

    // Find and run recipe, parse JSON output
    let recipes_dir = monorepo_dir.join("distro-builder/recipes");
    let recipe_bin = find_recipe(&monorepo_dir)?;
    let ctx = run_recipe_json_with_defines(
        &recipe_bin.path,
        &recipe_path,
        &downloads_dir,
        &defines,
        Some(&recipes_dir),
    )?;

    // Extract paths from ctx (recipe sets these)
    let output_dir = crate::artifact_store::central_output_dir_for_distro(base_dir);

    let source = ctx["source_path"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // Fallback: use SSOT version from distro-spec
            downloads_dir.join(kernel_source.source_dir_name())
        });

    let version = ctx["kernel_version"]
        .as_str()
        .map(|s: &str| s.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let vmlinuz = output_dir.join("staging/boot/vmlinuz");

    Ok(LinuxPaths {
        source,
        vmlinuz,
        version,
    })
}

/// Check if Linux source is available (without running the full recipe).
pub fn has_linux_source(base_dir: &Path) -> bool {
    // Check downloads for any tarball-extracted source (linux-*)
    let downloads = base_dir.join("downloads");
    if let Ok(entries) = std::fs::read_dir(&downloads) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str() {
                if name_str.starts_with("linux-") && entry.path().join("Makefile").exists() {
                    return true;
                }
            }
        }
    }

    false
}
