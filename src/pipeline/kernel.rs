//! The kernel artifacts of a variant: where the kernel build puts them, and
//! the Recipe check that they are installed.

use anyhow::{bail, Context, Result};
use distro_contract::Variant;
use std::path::{Path, PathBuf};

use crate::recipe::recipe_work_dir;

/// The recipe that installs a variant's kernel (the prebuilt release bundle).
pub const KERNEL_RECIPE: &str = "distro/builder/recipes/linux-prebuilt.rhai";
/// `kernel.release` under the kernel output directory.
pub const KERNEL_RELEASE_PATH: &str = "kernel-build/include/config/kernel.release";
/// The kernel image under the kernel output directory.
pub const KERNEL_IMAGE_PATH: &str = "staging/boot/vmlinuz";
/// The directory under the kernel output directory that holds `<kernel.release>/`
/// with the modules (`/usr/lib/modules` in the image).
pub const KERNEL_MODULES_DIR: &str = "staging/usr/lib/modules";

/// The kernel configuration of `variant` and its optional `kconfig.d` fragments.
pub fn kconfig_inputs(variant: &Variant) -> (PathBuf, Option<PathBuf>) {
    let fragments = variant.path("kconfig.d");
    (variant.path("kconfig"), fragments.is_dir().then_some(fragments))
}

/// Ask the kernel recipe whether `kernel_output_dir` holds the variant's kernel.
pub fn check_kernel_installed(variant: &Variant, kernel_output_dir: &Path) -> Result<()> {
    let repo_root = &variant.repo_root;
    let recipe_script = repo_root.join(KERNEL_RECIPE);
    let recipes_path = recipe_script
        .parent()
        .context("the kernel recipe has no parent directory")?
        .to_path_buf();
    let build_dir = work_dir_for_distro(repo_root, &variant.id)?;
    let (kconfig, fragments) = kconfig_inputs(variant);

    let recipe_bin = crate::recipe::find_recipe(repo_root)
        .context("Resolving recipe binary for build kernel")?;
    let defines = [("KERNEL_TARGET", variant.id.as_str())];
    // The kernel output is a symlink to the selected bundle; Recipe stages
    // only real files and directories, so pass the bundle itself.
    let target = std::fs::canonicalize(kernel_output_dir)
        .unwrap_or_else(|_| kernel_output_dir.to_path_buf());
    let mut inputs: Vec<(&str, &Path)> = vec![("kconfig", &kconfig), ("target", &target)];
    if let Some(fragments) = &fragments {
        inputs.push(("kconfig_fragments", fragments));
    }
    crate::recipe::run_recipe_phase_json_with_inputs(
        &recipe_bin.path,
        "isinstalled",
        &recipe_script,
        &build_dir,
        &defines,
        &inputs,
        &[],
        None,
        Some(&recipes_path),
    )
    .context("build kernel isinstalled check failed")?;

    Ok(())
}

/// Fail unless the variant's kernel is already installed: release builds
/// never build a kernel.
pub fn ensure_kernel_preinstalled(variant: &Variant, kernel_output_dir: &Path) -> Result<()> {
    if let Err(e) = check_kernel_installed(variant, kernel_output_dir) {
        bail!(
            "the {id} kernel is not installed: {e}\n\
             Kernel rebuilds are forbidden during release ISO builds.\n\
             Remediation: run 'cargo xtask build kernel {id}' (add '--rebuild' if provenance is stale, or use '--prebuilt'), then retry the ISO build.",
            id = variant.id,
        );
    }
    Ok(())
}

fn work_dir_for_distro(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let build_dir = recipe_work_dir(repo_root, distro_id)?;
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating kernel recipe work directory '{}'",
            build_dir.display()
        )
    })?;
    Ok(build_dir)
}
