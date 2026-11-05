//! Shared Linux kernel recipe wrapper.

use super::{find_recipe, run_recipe_json_with_defines};
use anyhow::Result;
use distro_spec::shared::KernelSource;
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

/// Run the linux.rhai recipe and return the output paths.
///
/// # Arguments
/// * `base_dir` - distro crate root (e.g., `/path/to/AcornOS`)
/// * `_kernel_source` - Legacy compatibility parameter; canonical kernel facts live in recipe
/// * `_module_install_path` - Legacy compatibility parameter; canonical module layout lives in recipe
pub fn linux(
    base_dir: &Path,
    _kernel_source: &KernelSource,
    _module_install_path: &str,
) -> Result<LinuxPaths> {
    let monorepo_dir = base_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base_dir.to_path_buf());

    let downloads_dir = base_dir.join("downloads");
    let kernel_artifact_root = monorepo_dir
        .join(".artifacts/kernel")
        .join(
            match base_dir.file_name().and_then(|s| s.to_str()).unwrap_or("") {
                "leviso" => "levitate",
                "AcornOS" => "acorn",
                other => other,
            },
        )
        .join("current")
        .to_string_lossy()
        .to_string();

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

    let canonical_kconfig_path = base_dir.join("build-host/kernel/kconfig");
    let legacy_kconfig_path = base_dir.join("kconfig");
    let kconfig_path = if canonical_kconfig_path.exists() {
        canonical_kconfig_path
    } else {
        legacy_kconfig_path
    }
    .to_string_lossy()
    .to_string();
    let defines: Vec<(&str, &str)> = vec![
        ("KERNEL_KCONFIG_PATH", &kconfig_path),
        ("KERNEL_ARTIFACT_ROOT", &kernel_artifact_root),
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

/// Legacy UI cache hint; this is not the recipe's acquisition/readiness check.
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
