//! Kernel building and installation.
//!
//! Shared infrastructure for building Linux kernels from source.
//! Used by both LevitateOS and AcornOS.
//!
//! # Usage
//!
//! ```rust,ignore
//! use distro_builder::build::kernel::{build_kernel, install_kernel};
//!
//! // Build kernel with custom kconfig
//! let kconfig = std::fs::read_to_string("kconfig")?;
//! let version = build_kernel(&kernel_source, &build_output, &kconfig)?;
//!
//! // Install to staging directory
//! install_kernel(&kernel_source, &build_output, &staging, &config)?;
//! ```

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::process::Cmd;
use distro_spec::shared::KernelSource;

// Re-export contracts from distro-contract
pub use distro_contract::kernel::{KernelBuildGuard, KernelGuard, KernelInstallConfig};

/// Download and extract a kernel tarball from cdn.kernel.org.
///
/// Downloads to `download_dir/linux-{version}.tar.xz`, verifies SHA256,
/// and extracts to `download_dir/linux-{version}/`.
///
/// Returns the path to the extracted kernel source directory.
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn download_kernel_tarball(source: &KernelSource, download_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(download_dir)?;

    let tarball_path = download_dir.join(source.tarball_filename());
    let extract_dir = download_dir.join(source.source_dir_name());

    // If already extracted, return immediately
    if extract_dir.join("Makefile").exists() {
        println!("  [SKIP] Kernel source already extracted at {}", extract_dir.display());
        return Ok(extract_dir);
    }

    // Download tarball if not cached
    if !tarball_path.exists() {
        let url = source.tarball_url();
        println!("  Downloading kernel {} ({} bytes)...", source.version, url);
        // -f: fail on HTTP errors, -S: show errors, -L: follow redirects
        // --retry 3: retry on transient failures
        // -C -: resume partial downloads
        // --progress-bar: show progress
        Cmd::new("curl")
            .args(["-fSL", "--retry", "3", "-C", "-", "--progress-bar", "-o"])
            .arg(tarball_path.to_str().unwrap())
            .arg(&url)
            .error_msg("Failed to download kernel tarball from cdn.kernel.org")
            .run_interactive()?;

        if !tarball_path.exists() {
            bail!("Download appeared to succeed but tarball not found at {}", tarball_path.display());
        }
    } else {
        println!("  [SKIP] Tarball already cached at {}", tarball_path.display());
    }

    // Verify SHA256 — always required, no placeholder bypass
    println!("  Verifying SHA256...");
    let tarball_bytes = fs::read(&tarball_path)
        .with_context(|| format!("Failed to read tarball at {}", tarball_path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&tarball_bytes);
    let actual_hash = format!("{:x}", hasher.finalize());
    if actual_hash != source.sha256 {
        let _ = fs::remove_file(&tarball_path);
        bail!(
            "SHA256 mismatch for linux-{}.tar.xz!\n\
             Expected: {}\n\
             Actual:   {}\n\n\
             The corrupted tarball has been removed. Re-run to download again.\n\
             If this persists, update the hash in distro-spec/src/shared/kernel.rs",
            source.version, source.sha256, actual_hash
        );
    }
    println!("  SHA256 OK: {}", &actual_hash[..16]);

    // Extract tarball
    println!("  Extracting kernel source (~1.4 GB uncompressed)...");
    Cmd::new("tar")
        .args(["xf"])
        .arg(tarball_path.to_str().unwrap())
        .args(["-C", download_dir.to_str().unwrap()])
        .error_msg("Failed to extract kernel tarball")
        .run()?;

    if !extract_dir.join("Makefile").exists() {
        bail!(
            "Kernel extraction failed: expected Makefile at {}\n\
             The tarball may have a different directory structure than expected.",
            extract_dir.display()
        );
    }

    println!("  Kernel {} source ready at {}", source.version, extract_dir.display());
    Ok(extract_dir)
}

/// Acquire kernel source, using cache if available.
///
/// Checks if the source is already extracted in `download_dir`, downloads if not.
/// This is the primary entry point for getting kernel source from a tarball.
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn acquire_kernel_source(source: &KernelSource, download_dir: &Path) -> Result<PathBuf> {
    let extract_dir = download_dir.join(source.source_dir_name());

    if extract_dir.join("Makefile").exists() {
        println!("Kernel source {} already available.", source.version);
        return Ok(extract_dir);
    }

    println!("Acquiring kernel source {}...", source.version);
    download_kernel_tarball(source, download_dir)
}

/// Build the kernel from source.
///
/// # Arguments
/// * `kernel_source` - Path to kernel source tree (must have Makefile)
/// * `output_dir` - Directory for build artifacts
/// * `kconfig` - Contents of the kconfig file with custom options
///
/// # Returns
/// The kernel version string (e.g., "6.12.0-levitate")
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn build_kernel(kernel_source: &Path, output_dir: &Path, kconfig: &str) -> Result<String> {
    println!("Building kernel from {}...", kernel_source.display());

    if !kernel_source.exists() {
        bail!(
            "Kernel source not found at {}\nUse acquire_kernel_source() to download from cdn.kernel.org",
            kernel_source.display()
        );
    }

    if !kernel_source.join("Makefile").exists() {
        bail!("Invalid kernel source - no Makefile found");
    }

    fs::create_dir_all(output_dir)?;
    let build_dir = output_dir.join("kernel-build");
    fs::create_dir_all(&build_dir)?;

    let config_path = build_dir.join(".config");
    let config_hash_path = build_dir.join(".config.kconfig-hash");

    let kernel_src_str = kernel_source.to_string_lossy();
    let build_dir_arg = format!("O={}", build_dir.display());

    // Compute hash of our kconfig
    let kconfig_hash = {
        let mut hasher = Sha256::new();
        hasher.update(kconfig.as_bytes());
        format!("{:x}", hasher.finalize())
    };

    // Check if we need to regenerate .config
    let need_config_regen = if config_path.exists() && config_hash_path.exists() {
        let cached_hash = fs::read_to_string(&config_hash_path).unwrap_or_default();
        cached_hash.trim() != kconfig_hash
    } else {
        true
    };

    if need_config_regen {
        // Start with x86_64 defconfig
        println!("  Generating base config from defconfig...");
        Cmd::new("make")
            .args(["-C", &kernel_src_str, &build_dir_arg, "x86_64_defconfig"])
            .error_msg("make defconfig failed")
            .run()?;

        // Apply our custom options
        println!("  Applying kernel config from kconfig...");
        apply_kernel_config(&config_path, kconfig)?;

        // Resolve dependencies
        println!("  Resolving config dependencies...");
        Cmd::new("make")
            .args(["-C", &kernel_src_str, &build_dir_arg, "olddefconfig"])
            .error_msg("make olddefconfig failed")
            .run()?;

        // Cache the kconfig hash
        fs::write(&config_hash_path, &kconfig_hash)?;
    } else {
        println!("  [SKIP] Config unchanged, reusing existing .config");
    }

    // Always run olddefconfig to handle new kernel options without prompting
    // This is needed even when kconfig is unchanged because the kernel source
    // may have been updated with new config options.
    println!("  Resolving any new config options...");
    Cmd::new("make")
        .args(["-C", &kernel_src_str, &build_dir_arg, "olddefconfig"])
        .error_msg("make olddefconfig failed")
        .run()?;

    let cpus = match std::thread::available_parallelism() {
        Ok(n) => n.get(),
        Err(e) => {
            eprintln!("  [WARN] Could not detect CPU count ({}), using 4 cores", e);
            4
        }
    };
    let jobs_arg = format!("-j{}", cpus);

    // Build kernel (interactive - user sees progress)
    // make will skip files that are already up-to-date
    println!("  Building kernel...");
    Cmd::new("make")
        .args(["-C", &kernel_src_str, &build_dir_arg, &jobs_arg])
        .error_msg("Kernel build failed")
        .run_interactive()?;

    // Build modules (interactive - user sees progress)
    println!("  Building modules...");
    Cmd::new("make")
        .args(["-C", &kernel_src_str, &build_dir_arg, &jobs_arg, "modules"])
        .error_msg("Module build failed")
        .run_interactive()?;

    let version = get_kernel_version(&build_dir)?;
    println!("  Kernel version: {}", version);

    Ok(version)
}

/// Apply kernel configuration options from kconfig content.
///
/// Merges custom kconfig options into an existing .config file,
/// replacing any existing values for the same keys.
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn apply_kernel_config(config_path: &Path, kconfig: &str) -> Result<()> {
    // FAIL FAST: If config file exists but is unreadable, that's a real error
    // Don't silently treat corrupted/unreadable config as empty
    let mut config = if config_path.exists() {
        fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read kernel config at {}", config_path.display()))?
    } else {
        String::new()
    };

    for line in kconfig.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if let Some((key, _value)) = line.split_once('=') {
            let pattern = format!("{}=", key);
            let pattern_not = format!("# {} is not set", key);
            config = config
                .lines()
                .filter(|l| !l.starts_with(&pattern) && !l.starts_with(&pattern_not))
                .collect::<Vec<_>>()
                .join("\n");

            config.push('\n');
            config.push_str(line);
        }
    }

    fs::write(config_path, config)?;
    Ok(())
}

/// Get the kernel version from the build directory.
pub fn get_kernel_version(build_dir: &Path) -> Result<String> {
    let release_path = build_dir.join("include/config/kernel.release");
    if release_path.exists() {
        return Ok(fs::read_to_string(&release_path)?.trim().to_string());
    }

    let makefile = build_dir.join("Makefile");
    if makefile.exists() {
        let content = fs::read_to_string(&makefile)?;
        let mut version = String::new();
        let mut patchlevel = String::new();
        let mut sublevel = String::new();
        let mut extraversion = String::new();

        for line in content.lines() {
            if let Some(v) = line.strip_prefix("VERSION = ") {
                version = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("PATCHLEVEL = ") {
                patchlevel = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("SUBLEVEL = ") {
                sublevel = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("EXTRAVERSION = ") {
                extraversion = v.trim().to_string();
            }
        }

        if !version.is_empty() && !patchlevel.is_empty() {
            return Ok(format!(
                "{}.{}.{}{}",
                version, patchlevel, sublevel, extraversion
            ));
        }
    }

    bail!("Could not determine kernel version")
}

/// Build kernel from a project's kconfig file.
///
/// This is the standard entry point for distro builders. It:
/// 1. Cleans up any stale kernel-build symlink (from previous theft mode)
/// 2. Reads the kconfig file from `base_dir/kconfig`
/// 3. Delegates to `build_kernel()`
///
/// # Arguments
/// * `kernel_source` - Path to kernel source tree
/// * `output_dir` - Directory for build artifacts
/// * `base_dir` - Project root (must contain a `kconfig` file)
///
/// # Returns
/// The kernel version string
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn build_kernel_from_kconfig(
    kernel_source: &Path,
    output_dir: &Path,
    base_dir: &Path,
) -> Result<String> {
    // Clean up any existing kernel-build symlink (from previous theft)
    let our_kernel_build = output_dir.join("kernel-build");
    if our_kernel_build.is_symlink() {
        fs::remove_file(&our_kernel_build).with_context(|| {
            format!(
                "Failed to remove stolen kernel symlink at {}",
                our_kernel_build.display()
            )
        })?;
        println!("  Removed stale kernel-build symlink (was stolen from leviso)");
    }

    let kconfig_path = base_dir.join("kconfig");
    if !kconfig_path.exists() {
        bail!(
            "Kernel config not found at {}\nExpected kconfig file in project root.",
            kconfig_path.display()
        );
    }
    let kconfig = fs::read_to_string(&kconfig_path)
        .with_context(|| format!("Failed to read {}", kconfig_path.display()))?;

    build_kernel(kernel_source, output_dir, &kconfig)
}

/// Install kernel and modules to staging directory.
///
/// # Arguments
/// * `kernel_source` - Path to kernel source tree
/// * `build_output` - Directory containing kernel-build/
/// * `staging` - Target staging directory
/// * `config` - Distro-specific installation config
///
/// # Returns
/// The kernel version string
#[deprecated(note = "Use distro_builder::recipe::linux::linux() instead — kernel builds should go through the recipe system")]
pub fn install_kernel(
    kernel_source: &Path,
    build_output: &Path,
    staging: &Path,
    config: &impl KernelInstallConfig,
) -> Result<String> {
    let build_dir = build_output.join("kernel-build");

    let vmlinux = build_dir.join("arch/x86/boot/bzImage");
    if !vmlinux.exists() {
        bail!(
            "Kernel not built. Run build_kernel() first.\nExpected: {}",
            vmlinux.display()
        );
    }

    let version = get_kernel_version(&build_dir)?;
    println!("Installing kernel {} to staging...", version);

    // Atomic Installation: Install to a temporary directory first
    let temp_staging = staging.parent().unwrap().join("staging.tmp");
    if temp_staging.exists() {
        fs::remove_dir_all(&temp_staging)?;
    }
    fs::create_dir_all(&temp_staging)?;

    let boot_dir = temp_staging.join("boot");
    fs::create_dir_all(&boot_dir)?;

    // Install kernel image
    let kernel_dest = boot_dir.join(config.kernel_filename());
    fs::copy(&vmlinux, &kernel_dest)?;
    println!("  Installed /boot/{}", config.kernel_filename());

    // Install modules
    let module_base = config.module_install_path().trim_start_matches('/');
    let modules_dir = temp_staging.join(module_base).join(&version);
    fs::create_dir_all(&modules_dir)?;

    println!(
        "  Installing modules to {}/{}...",
        config.module_install_path(),
        version
    );
    Cmd::new("make")
        .args(["-C", &kernel_source.to_string_lossy()])
        .arg(format!("O={}", build_dir.display()))
        .arg(format!("INSTALL_MOD_PATH={}", temp_staging.display()))
        .arg("modules_install")
        .error_msg("Module install failed")
        .run_interactive()?;

    // Handle UsrMerge: make modules_install puts files in /lib/modules,
    // but we may want them in /usr/lib/modules.
    let lib_modules = temp_staging.join("lib/modules");
    let target_modules = temp_staging.join(module_base);

    if lib_modules.exists() && module_base != "lib/modules" {
        println!(
            "  Moving modules from lib/modules to {}...",
            config.module_install_path()
        );
        fs::create_dir_all(&target_modules)?;

        for entry in fs::read_dir(&lib_modules)? {
            let entry = entry?;
            let name = entry.file_name();
            let src = entry.path();
            let dst = target_modules.join(&name);

            if dst.exists() {
                fs::remove_dir_all(&dst)?;
            }
            fs::rename(&src, &dst)?;
        }
        // Remove the empty lib/modules
        let _ = fs::remove_dir_all(&lib_modules);
        // Remove lib if empty
        let _ = fs::remove_dir(temp_staging.join("lib"));
    }

    // Remove symlinks to build directories (not needed in rootfs)
    let final_modules_dir = target_modules.join(&version);
    let _ = fs::remove_file(final_modules_dir.join("source"));
    let _ = fs::remove_file(final_modules_dir.join("build"));

    // Count installed modules
    let mut module_count = 0;
    let mut walk_errors = 0;
    for entry in walkdir::WalkDir::new(&final_modules_dir) {
        match entry {
            Ok(e) => {
                if e.path()
                    .extension()
                    .map(|ext| ext == "ko" || ext == "xz" || ext == "gz")
                    .unwrap_or(false)
                {
                    module_count += 1;
                }
            }
            Err(e) => {
                walk_errors += 1;
                eprintln!("  [WARN] Error reading module entry: {}", e);
            }
        }
    }
    if walk_errors > 0 {
        eprintln!(
            "  [WARN] {} errors encountered while counting modules (count may be inaccurate)",
            walk_errors
        );
    }
    println!("  Installed {} kernel modules", module_count);

    // Final Integrity Check
    if !final_modules_dir.exists() {
        bail!(
            "Kernel installation failed: modules directory for version {} not found in staging",
            version
        );
    }

    // Atomic Swap: rename temp_staging to staging
    if staging.exists() {
        fs::remove_dir_all(staging)?;
    }
    fs::rename(&temp_staging, staging)?;

    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_apply_kernel_config_new_options() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".config");

        // Start with empty config
        fs::write(&config_path, "CONFIG_FOO=y\nCONFIG_BAR=n\n").unwrap();

        // Apply new options
        let kconfig = "CONFIG_BAR=y\nCONFIG_BAZ=m\n";
        apply_kernel_config(&config_path, kconfig).unwrap();

        let result = fs::read_to_string(&config_path).unwrap();
        assert!(result.contains("CONFIG_FOO=y"));
        assert!(result.contains("CONFIG_BAR=y"));
        assert!(result.contains("CONFIG_BAZ=m"));
        // Should not have duplicate CONFIG_BAR
        assert_eq!(result.matches("CONFIG_BAR").count(), 1);
    }

    #[test]
    fn test_apply_kernel_config_comments_ignored() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".config");

        fs::write(&config_path, "").unwrap();

        let kconfig = "# This is a comment\nCONFIG_TEST=y\n\n# Another comment\n";
        apply_kernel_config(&config_path, kconfig).unwrap();

        let result = fs::read_to_string(&config_path).unwrap();
        assert!(result.contains("CONFIG_TEST=y"));
        assert!(!result.contains("# This is a comment"));
    }
}
