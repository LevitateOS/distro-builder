//! Shared recipe binary resolution and execution.
//!
//! Recipe is the general-purpose package manager used by Alpine-based distros
//! to manage build dependencies.
//!
//! Resolution order:
//! 1. System PATH (`which recipe`)
//! 2. Monorepo submodule (`../tools/recipe`)
//! 3. `RECIPE_BIN` env var (path to binary)
//! 4. `RECIPE_SRC` env var (path to source, will build)

pub mod alpine;
pub mod linux;

use crate::process::ensure_exists;
use anyhow::{bail, Context, Result};
use distro_spec::shared::LEVITATE_CARGO_TOOLS;
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Extract the distro directory name from a base_dir path.
///
/// e.g., `/home/user/LevitateOS/AcornOS` → `"AcornOS"`
fn distro_name(base_dir: &Path) -> &str {
    base_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

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

    /// Run a recipe file with this binary.
    pub fn run(&self, recipe_path: &Path, build_dir: &Path) -> Result<()> {
        run_recipe(&self.path, recipe_path, build_dir, None)
    }

    /// Run a recipe file with this binary, passing a recipes search path.
    pub fn run_with_recipes_path(
        &self,
        recipe_path: &Path,
        build_dir: &Path,
        recipes_path: Option<&Path>,
    ) -> Result<()> {
        run_recipe(&self.path, recipe_path, build_dir, recipes_path)
    }
}

/// Find the recipe binary using the resolution order.
pub fn find_recipe(monorepo_dir: &Path) -> Result<RecipeBinary> {
    // 1. Check system PATH
    if let Ok(path) = which::which("recipe") {
        return Ok(RecipeBinary { path });
    }

    // 2. Check monorepo submodule
    let submodule = monorepo_dir.join("tools/recipe");
    if submodule.join("Cargo.toml").exists() {
        return build_from_source(&submodule, monorepo_dir, RecipeSource::Monorepo);
    }

    // 3. Check RECIPE_BIN env var
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

    // 4. Check RECIPE_SRC env var
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

    bail!(
        "Could not find recipe binary.\n\n\
         Resolution order tried:\n\
         1. System PATH - not found\n\
         2. Monorepo at {} - not found\n\
         3. RECIPE_BIN env var - not set\n\
         4. RECIPE_SRC env var - not set\n\n\
         Solutions:\n\
         - Install recipe to PATH\n\
         - Set RECIPE_BIN=/path/to/recipe\n\
         - Set RECIPE_SRC=/path/to/recipe/source",
        submodule.display()
    )
}

/// Build recipe from source.
fn build_from_source(
    crate_path: &Path,
    monorepo_dir: &Path,
    source: RecipeSource,
) -> Result<RecipeBinary> {
    let release_build = env::var("RECIPE_BUILD_RELEASE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

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

/// Run a recipe using the recipe binary, returning the ctx as JSON.
pub fn run_recipe_json(
    recipe_bin: &Path,
    recipe_path: &Path,
    build_dir: &Path,
) -> Result<serde_json::Value> {
    run_recipe_json_with_defines(recipe_bin, recipe_path, build_dir, &[], None)
}

/// Run a recipe with user-defined scope constants injected via --define.
pub fn run_recipe_json_with_defines(
    recipe_bin: &Path,
    recipe_path: &Path,
    build_dir: &Path,
    defines: &[(&str, &str)],
    recipes_path: Option<&Path>,
) -> Result<serde_json::Value> {
    eprintln!("  Running recipe: {}", recipe_path.display());
    eprintln!("    Build dir: {}", build_dir.display());

    let mut cmd = Command::new(recipe_bin);
    cmd.arg("install")
        .arg(recipe_path)
        .arg("--build-dir")
        .arg(build_dir);

    if let Some(rp) = recipes_path {
        cmd.arg("--recipes-path").arg(rp);
    }

    for (key, value) in defines {
        cmd.arg("--define").arg(format!("{}={}", key, value));
    }

    let output = cmd
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("Failed to execute recipe: {}", recipe_bin.display()))?;

    if !output.status.success() {
        bail!(
            "Recipe failed with exit code: {}",
            output.status.code().unwrap_or(-1)
        );
    }

    let ctx: serde_json::Value = serde_json::from_slice(&output.stdout)
        .with_context(|| "Failed to parse recipe JSON output")?;

    Ok(ctx)
}

/// Run a recipe using the recipe binary (legacy, no JSON parsing).
pub fn run_recipe(
    recipe_bin: &Path,
    recipe_path: &Path,
    build_dir: &Path,
    recipes_path: Option<&Path>,
) -> Result<()> {
    run_recipe_json_with_defines(recipe_bin, recipe_path, build_dir, &[], recipes_path)?;
    Ok(())
}

/// Run the tool recipes to install recstrap, recfstab, recchroot to staging.
///
/// # Arguments
/// * `base_dir` - distro crate root (e.g., `/path/to/AcornOS`)
pub fn install_tools(base_dir: &Path) -> Result<()> {
    let distro_name = distro_name(base_dir);
    let monorepo_dir = base_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base_dir.to_path_buf());

    let downloads_dir = base_dir.join("downloads");
    let staging_bin = base_dir.join("output/staging/usr/bin");

    // Find recipe binary once
    let recipe_bin = find_recipe(&monorepo_dir)?;

    // Run each tool recipe
    for tool in LEVITATE_CARGO_TOOLS {
        let recipe_path = base_dir.join(format!("deps/{}.rhai", tool));
        let installed_path = staging_bin.join(tool);

        // Skip if already installed
        if installed_path.exists() {
            println!("  {} already installed", tool);
            continue;
        }

        ensure_exists(&recipe_path, &format!("{} recipe", tool)).map_err(|_| {
            anyhow::anyhow!(
                "{} recipe not found at: {}\n\
                 Expected {}.rhai in {}/deps/",
                tool,
                recipe_path.display(),
                tool,
                distro_name
            )
        })?;

        recipe_bin.run(&recipe_path, &downloads_dir)?;

        // Verify installation
        if !installed_path.exists() {
            bail!(
                "Recipe completed but {} not found at: {}",
                tool,
                installed_path.display()
            );
        }
    }

    Ok(())
}

/// Run the packages.rhai recipe to extract and install Alpine packages into rootfs.
///
/// # Arguments
/// * `base_dir` - distro crate root
pub fn packages(base_dir: &Path) -> Result<()> {
    let distro_name = distro_name(base_dir);
    let monorepo_dir = base_dir
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| base_dir.to_path_buf());

    let downloads_dir = base_dir.join("downloads");
    let recipe_path = base_dir.join("deps/packages.rhai");

    ensure_exists(&recipe_path, "Packages recipe").map_err(|_| {
        anyhow::anyhow!(
            "Packages recipe not found at: {}\n\
             Expected packages.rhai in {}/deps/",
            recipe_path.display(),
            distro_name
        )
    })?;

    // Verify alpine.rhai has been run first
    let rootfs = downloads_dir.join("rootfs");

    if !rootfs.join("usr").exists() {
        bail!(
            "rootfs not found at: {}\n\
             Run alpine.rhai first (via alpine() function).",
            rootfs.display()
        );
    }

    // Find and run recipe
    let recipe_bin = find_recipe(&monorepo_dir)?;
    recipe_bin.run(&recipe_path, &downloads_dir)?;

    Ok(())
}

/// Clear the recipe cache directory (~/.cache/levitate/).
pub fn clear_cache() -> Result<()> {
    let cache_dir = dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("levitate");

    if cache_dir.exists() {
        std::fs::remove_dir_all(&cache_dir)?;
        std::fs::create_dir_all(&cache_dir)?;
    }
    Ok(())
}
