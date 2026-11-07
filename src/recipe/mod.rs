//! Shared recipe binary resolution and execution.
//!
//! Recipe is the general-purpose package manager used by Alpine-based distros
//! to manage build dependencies.
//!
//! Resolution order:
//! 1. `RECIPE_BIN` env var (path to binary)
//! 2. `RECIPE_SRC` env var (path to source, will build)
//! 3. Workspace binary under `target/{debug,release}/recipe`
//! 4. Monorepo submodule (`../tools/recipe`, built from source if needed)
//! 5. System PATH (`which recipe`)

pub mod linux;
pub(crate) mod outputs;
pub mod qemu;
pub mod rootfs_source;
pub mod tools;

pub use crate::pipeline::paths::recipe_work_dir;

use anyhow::{bail, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::{io, os::unix::process::CommandExt};

/// How the recipe binary was built from source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecipeSource {
    /// Built from monorepo submodule.
    Monorepo,
    /// Built from source via RECIPE_SRC.
    EnvSrc,
}

/// Resolved recipe binary.
#[derive(Debug, Clone)]
pub struct RecipeBinary {
    /// Path to the binary.
    pub path: PathBuf,
}

impl RecipeBinary {
    /// Check if the binary exists and is executable.
    pub fn is_valid(&self) -> bool {
        if !self.path.exists() {
            return false;
        }

        match std::fs::metadata(&self.path) {
            Ok(meta) => {
                if !meta.is_file() {
                    return false;
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = meta.permissions().mode();
                    if mode & 0o111 == 0 {
                        return false;
                    }
                }
                true
            }
            Err(_) => false,
        }
    }
}

/// Find the recipe binary using the resolution order.
pub fn find_recipe(monorepo_dir: &Path) -> Result<RecipeBinary> {
    let submodule = monorepo_dir.join("tools/recipe");

    // 1. Check RECIPE_BIN env var
    if let Ok(bin_path) = env::var("RECIPE_BIN") {
        let path = PathBuf::from(&bin_path);
        if path.exists() {
            let binary = RecipeBinary { path };
            if binary.is_valid() {
                return Ok(binary);
            }
            bail!(
                "RECIPE_BIN points to invalid binary: {}\n\
                 File exists but is not executable.",
                bin_path
            );
        }
        bail!("RECIPE_BIN points to non-existent path: {}", bin_path);
    }

    // 2. Check RECIPE_SRC env var
    if let Ok(src_path) = env::var("RECIPE_SRC") {
        let src = PathBuf::from(&src_path);
        if src.join("Cargo.toml").exists() {
            let workspace_root = src.parent().unwrap_or(&src);
            return build_from_source(&src, workspace_root, RecipeSource::EnvSrc);
        }
        bail!(
            "RECIPE_SRC is not a valid Cargo crate: {}\n\
             Expected Cargo.toml at that path.",
            src_path
        );
    }

    // 3. Prefer workspace recipe binaries over global PATH installs.
    // This avoids stale global binaries missing helpers required by this repo.
    let prefer_release = recipe_build_release();
    let preferred_profile = if prefer_release { "release" } else { "debug" };
    let fallback_profile = if prefer_release { "debug" } else { "release" };

    let preferred_binary = RecipeBinary {
        path: monorepo_dir
            .join("target")
            .join(preferred_profile)
            .join("recipe"),
    };
    if preferred_binary.is_valid() {
        return Ok(preferred_binary);
    }

    let fallback_binary = RecipeBinary {
        path: monorepo_dir
            .join("target")
            .join(fallback_profile)
            .join("recipe"),
    };
    if fallback_binary.is_valid() {
        return Ok(fallback_binary);
    }

    // 4. Check monorepo submodule and build if needed.
    if submodule.join("Cargo.toml").exists() {
        return build_from_source(&submodule, monorepo_dir, RecipeSource::Monorepo);
    }

    // 5. Check system PATH as final fallback.
    if let Ok(path) = which::which("recipe") {
        return Ok(RecipeBinary { path });
    }

    bail!(
        "Could not find recipe binary.\n\n\
         Resolution order tried:\n\
         1. RECIPE_BIN env var - not set\n\
         2. RECIPE_SRC env var - not set\n\
         3. Workspace binary under {} - not found\n\
         4. Monorepo at {} - not found\n\
         5. System PATH - not found\n\n\
         Solutions:\n\
         - Set RECIPE_BIN=/path/to/recipe\n\
         - Set RECIPE_SRC=/path/to/recipe/source\n\
         - Ensure tools/recipe is checked out in this monorepo\n\
         - Install recipe to PATH",
        monorepo_dir.join("target").display(),
        submodule.display()
    )
}

/// Build recipe from source.
fn build_from_source(
    crate_path: &Path,
    monorepo_dir: &Path,
    source: RecipeSource,
) -> Result<RecipeBinary> {
    let release_build = recipe_build_release();

    let source_desc = match source {
        RecipeSource::Monorepo => "monorepo",
        RecipeSource::EnvSrc => "RECIPE_SRC",
    };

    println!("  Building recipe ({})...", source_desc);
    println!("    Source: {}", crate_path.display());

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--package")
        .arg("levitate-recipe")
        .current_dir(crate_path);

    if release_build {
        cmd.arg("--release");
        println!("    Profile: release");
    } else {
        println!("    Profile: debug");
    }

    let output = cmd
        .output()
        .with_context(|| "Failed to execute cargo build for recipe".to_string())?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "cargo build failed for recipe\n  Exit code: {}\n  stderr: {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        );
    }

    let profile = if release_build { "release" } else { "debug" };

    // In a workspace, binary goes to workspace root's target directory
    let binary = monorepo_dir.join("target").join(profile).join("recipe");

    if !binary.exists() {
        // Fallback: check crate's local target (non-workspace case)
        let local_binary = crate_path.join("target").join(profile).join("recipe");
        if local_binary.exists() {
            println!("    Built: {}", local_binary.display());
            return Ok(RecipeBinary { path: local_binary });
        }
        bail!(
            "Built binary not found at:\n  - {}\n  - {}",
            binary.display(),
            local_binary.display()
        );
    }

    println!("    Built: {}", binary.display());

    Ok(RecipeBinary { path: binary })
}

fn recipe_build_release() -> bool {
    env::var("RECIPE_BUILD_RELEASE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}

/// Run a specific recipe lifecycle command (`install`, `isinstalled`, etc),
/// returning the ctx as JSON.
pub fn run_recipe_phase_json_with_defines(
    recipe_bin: &Path,
    phase: &str,
    recipe_path: &Path,
    build_dir: &Path,
    defines: &[(&str, &str)],
    recipes_path: Option<&Path>,
) -> Result<serde_json::Value> {
    run_recipe_phase_json_impl(
        recipe_bin,
        phase,
        recipe_path,
        build_dir,
        defines,
        recipes_path,
        &[],
        &[],
        None,
        false,
        None,
    )
}

/// Run a managed API 2 recipe with explicit read-only file/tree inputs. State
/// belongs to this artifact work directory; returned outputs are published host
/// paths, not worker paths or guesses under build_dir.
pub fn run_recipe_phase_json_with_inputs(
    recipe_bin: &Path,
    phase: &str,
    recipe_path: &Path,
    build_dir: &Path,
    defines: &[(&str, &str)],
    inputs: &[(&str, &Path)],
    network_hosts: &[String],
    memory_mib: Option<u64>,
    recipes_path: Option<&Path>,
) -> Result<serde_json::Value> {
    run_recipe_phase_json_impl(
        recipe_bin,
        phase,
        recipe_path,
        build_dir,
        defines,
        recipes_path,
        inputs,
        network_hosts,
        memory_mib,
        false,
        None,
    )
}

fn run_recipe_phase_json_impl(
    recipe_bin: &Path,
    phase: &str,
    recipe_path: &Path,
    build_dir: &Path,
    defines: &[(&str, &str)],
    recipes_path: Option<&Path>,
    inputs: &[(&str, &Path)],
    network_hosts: &[String],
    memory_mib: Option<u64>,
    force_acquire: bool,
    storage_max_objects: Option<u64>,
) -> Result<serde_json::Value> {
    eprintln!("  Running recipe: {}", recipe_path.display());
    eprintln!("    Phase: {}", phase);
    eprintln!("    Build dir: {}", build_dir.display());

    std::fs::create_dir_all(build_dir)
        .with_context(|| format!("creating recipe work directory '{}'", build_dir.display()))?;
    // A successful subprocess must provide this invocation's result. Never
    // accept a stale .recipe-output.json from an earlier invocation.
    let result_file = tempfile::Builder::new()
        .prefix(".recipe-result.")
        .suffix(".json")
        .tempfile_in(build_dir)
        .context("creating fresh recipe result file")?;
    let json_path = result_file.path();

    let mut cmd = Command::new(recipe_bin);
    cmd.arg(phase)
        .arg(recipe_path)
        .arg("--build-dir")
        .arg(build_dir)
        .arg("--json-output")
        .arg(&json_path);
    cmd.arg("--state-dir").arg(build_dir.join("recipe-state"));
    if force_acquire {
        cmd.args(["--force-acquire", "--force-install"]);
    }
    if let Some(objects) = storage_max_objects {
        cmd.arg("--storage-max-objects").arg(objects.to_string());
    }
    if let Some(memory_mib) = memory_mib {
        cmd.arg("--memory-mib").arg(memory_mib.to_string());
    }
    for host in network_hosts {
        cmd.arg("--network-host").arg(host);
    }
    for (name, path) in inputs {
        let path = path.to_str().context("recipe input path is not UTF-8")?;
        cmd.arg("--input").arg(format!("{name}={path}"));
    }

    if let Some(rp) = recipes_path {
        cmd.arg("--recipes-path").arg(rp);
    }

    for (key, value) in defines {
        cmd.arg("--define").arg(format!("{}={}", key, value));
    }

    #[cfg(unix)]
    {
        // Ensure recipe subprocesses cannot outlive distro-builder when parent gets cancelled.
        // This prevents orphaned recipe processes from keeping recipe locks after cancellation.
        unsafe {
            cmd.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let mut child = cmd
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("Failed to execute recipe: {}", recipe_bin.display()))?;
    let status = child
        .wait()
        .with_context(|| format!("Failed to execute recipe: {}", recipe_bin.display()))?;

    if !status.success() {
        bail!(
            "Recipe failed with exit code: {}",
            status.code().unwrap_or(-1)
        );
    }

    let result =
        std::fs::read(json_path).context("reading this invocation's recipe JSON result")?;
    let ctx: serde_json::Value = serde_json::from_slice(&result)
        .context("failed to parse this invocation's recipe JSON result")?;
    if !ctx.is_object() {
        bail!("recipe result must be a ctx JSON object");
    }
    Ok(ctx)
}
