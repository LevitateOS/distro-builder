//! Shared Linux kernel recipe wrapper.

use super::{recipe_work_dir, Invocation, RecipeResult};
use anyhow::Result;
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
    let bundle = distro_contract::load(repo_root, distro_id)?;
    let monorepo_dir = &bundle.repo_root;
    let work_dir = recipe_work_dir(monorepo_dir, distro_id)?;
    // Single SSOT kernel recipe for all distros (parity). Distro-specific overrides
    // (e.g. <distro>/deps/linux.rhai) are intentionally ignored.
    let shared_recipe = monorepo_dir.join("distro/builder/recipes/linux.rhai");
    if !shared_recipe.exists() {
        anyhow::bail!(
            "Linux recipe not found at expected shared path:\n  - {}",
            shared_recipe.display()
        );
    }
    let recipe_path = shared_recipe;

    let (kconfig_path, fragments) = crate::kernel::kconfig_inputs(&bundle);
    let current = distro_contract::runs::kernel_dir(monorepo_dir, distro_id);
    let mut invocation = Invocation::new(monorepo_dir, "install", &recipe_path, &work_dir)
        .define("KERNEL_TARGET", distro_id)
        .input("kconfig", &kconfig_path);
    if let Some(fragments) = &fragments {
        invocation = invocation.input("kconfig_fragments", fragments);
    }
    if current.is_dir() {
        invocation = invocation.input("target", &current);
    }
    let ctx = invocation.run()?;

    select_kernel_bundle(&ctx, &current)?;
    let vmlinuz = ctx.file("kernel")?;
    // Modules are a required companion product, even though callers currently
    // consume only the image path from this return type.
    ctx.directory("modules")?;
    let source = ctx.optional_directory("source")?;
    let version = ctx.metadata_string("kernel_version")?.to_owned();
    Ok(LinuxPaths {
        source,
        vmlinuz,
        version,
    })
}

/// Select the explicit immutable bundle returned by Recipe for development
/// consumers. Existing physical artifact trees are retained, never overwritten.
pub fn select_kernel_bundle(ctx: &RecipeResult, destination: &Path) -> Result<()> {
    let bundle = ctx.directory("bundle")?;
    ctx.file("kernel")?;
    ctx.directory("modules")?;
    super::outputs::select_directory(&bundle, destination)
}
