//! Kernel module operations - copying, depmod.
//!
//! Copies kernel modules from the staging directory and runs depmod.

use anyhow::{bail, Result};
use std::fs;
use std::path::Path;

use super::context::BuildContext;
use crate::process::Cmd;

/// Copy kernel modules to the final staging directory.
///
/// Modules are already installed to output/staging/lib/modules/ by the kernel build.
/// This function copies them to the EROFS staging root and runs depmod.
///
/// # Arguments
/// * `ctx` - Build context
/// * `build_cmd_hint` - Command hint for error messages (e.g., "acornos build kernel")
/// * `metadata_files` - List of module metadata files to copy (e.g., modules.dep, modules.alias)
pub fn copy_modules(
    ctx: &BuildContext,
    build_cmd_hint: &str,
    metadata_files: &[&str],
) -> Result<()> {
    println!("Setting up kernel modules...");

    // Modules are installed to output/staging/lib/modules/ during kernel build
    let modules_base = ctx.output.join("staging/lib/modules");

    if !modules_base.exists() {
        bail!(
            "No kernel modules found at {}.\n\
             Run '{}' first.",
            modules_base.display(),
            build_cmd_hint,
        );
    }

    let kernel_version = find_kernel_version(&modules_base)?;
    println!("  Kernel version: {}", kernel_version);

    let src_modules = modules_base.join(&kernel_version);
    let dst_modules = ctx.staging.join("lib/modules").join(&kernel_version);
    fs::create_dir_all(&dst_modules)?;

    // Copy entire modules directory
    copy_modules_recursive(&src_modules, &dst_modules)?;

    // Copy module metadata files
    println!("  Copying module metadata for modprobe...");
    for metadata_file in metadata_files {
        let src = src_modules.join(metadata_file);
        if src.exists() {
            fs::copy(&src, dst_modules.join(metadata_file))?;
        }
    }

    // Run depmod
    run_depmod_internal(&ctx.staging, &kernel_version)?;

    Ok(())
}

/// Run depmod to regenerate module dependencies.
pub fn run_depmod(ctx: &BuildContext) -> Result<()> {
    let modules_base = ctx.staging.join("lib/modules");
    let kernel_version = find_kernel_version(&modules_base)?;
    run_depmod_internal(&ctx.staging, &kernel_version)
}

fn run_depmod_internal(staging: &Path, kernel_version: &str) -> Result<()> {
    println!("  Running depmod...");
    Cmd::new("depmod")
        .args(["-a", "-b"])
        .arg_path(staging)
        .arg(kernel_version)
        .error_msg("depmod failed. Install: sudo dnf install kmod")
        .run()?;
    println!("  depmod completed successfully");
    Ok(())
}

/// Find the kernel version from the modules directory.
fn find_kernel_version(modules_base: &Path) -> Result<String> {
    for entry in fs::read_dir(modules_base)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.contains('.') && entry.path().is_dir() {
            return Ok(name_str.to_string());
        }
    }
    bail!("Could not find kernel modules directory")
}

/// Copy entire modules directory recursively.
fn copy_modules_recursive(src: &Path, dst: &Path) -> Result<()> {
    let mut module_count = 0;

    // Copy the kernel/ subdirectory which contains all modules
    let kernel_src = src.join("kernel");
    let kernel_dst = dst.join("kernel");

    if kernel_src.exists() {
        copy_dir_recursive(&kernel_src, &kernel_dst, &mut module_count)?;
    }

    println!("  Copied {} kernel modules", module_count);
    Ok(())
}

/// Recursively copy a directory, counting .ko files.
fn copy_dir_recursive(src: &Path, dst: &Path, count: &mut usize) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());

        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path, count)?;
        } else {
            fs::copy(&path, &dest_path)?;
            // Count .ko files (with any compression extension)
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if name.contains(".ko") {
                *count += 1;
            }
        }
    }

    Ok(())
}
