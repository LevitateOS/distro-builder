use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// 00Build kernel declaration fields required to verify/install through Recipe.
#[derive(Debug, Clone)]
pub struct S00BuildKernelSpec {
    pub recipe_kernel_script: String,
    pub kernel_version: String,
    pub kernel_sha256: String,
    pub kernel_localversion: String,
    pub module_install_path: String,
}

/// 00Build evidence script declaration.
#[derive(Debug, Clone)]
pub struct S00BuildEvidenceSpec {
    pub script_path: String,
    pub pass_marker: String,
    pub kernel_release_path: String,
    pub kernel_image_path: String,
    pub iso_filename: String,
}

/// Outcome of 00Build kernel ensure operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S00BuildKernelEnsureOutcome {
    AlreadyInstalled,
    InstalledNow,
}

/// Check 00Build kernel install state using Recipe `isinstalled`.
///
/// This does not build or rebuild anything.
pub fn check_kernel_installed_via_recipe(
    repo_root: &Path,
    output_dir: &Path,
    spec: &S00BuildKernelSpec,
) -> Result<()> {
    let recipe_script = repo_root.join(&spec.recipe_kernel_script);
    let recipes_path = recipe_script
        .parent()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "recipe_kernel_script has no parent directory: {}",
                recipe_script.display()
            )
        })?
        .to_path_buf();
    let build_dir = build_dir_for_output_dir(repo_root, output_dir)?;

    let recipe_bin =
        crate::recipe::find_recipe(repo_root).context("Resolving recipe binary for 00Build")?;
    let defines = kernel_defines(spec);
    crate::recipe::run_recipe_phase_json_with_defines(
        &recipe_bin.path,
        "isinstalled",
        &recipe_script,
        &build_dir,
        &defines,
        Some(&recipes_path),
    )
    .context("00Build kernel isinstalled check failed")?;

    Ok(())
}

/// Ensure 00Build kernel install state through shared Recipe orchestration.
///
/// Behavior:
/// - first runs `recipe isinstalled` (no build);
/// - if missing, runs `recipe install` with `KERNEL_FORCE_REBUILD=0`;
/// - re-runs `recipe isinstalled` to confirm.
pub fn ensure_kernel_installed_via_recipe(
    repo_root: &Path,
    output_dir: &Path,
    spec: &S00BuildKernelSpec,
) -> Result<S00BuildKernelEnsureOutcome> {
    if check_kernel_installed_via_recipe(repo_root, output_dir, spec).is_ok() {
        return Ok(S00BuildKernelEnsureOutcome::AlreadyInstalled);
    }

    let recipe_script = repo_root.join(&spec.recipe_kernel_script);
    let recipes_path = recipe_script
        .parent()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "recipe_kernel_script has no parent directory: {}",
                recipe_script.display()
            )
        })?
        .to_path_buf();
    let build_dir = build_dir_for_output_dir(repo_root, output_dir)?;

    let recipe_bin =
        crate::recipe::find_recipe(repo_root).context("Resolving recipe binary for 00Build")?;
    let defines = kernel_defines(spec);
    crate::recipe::run_recipe_phase_json_with_defines(
        &recipe_bin.path,
        "install",
        &recipe_script,
        &build_dir,
        &defines,
        Some(&recipes_path),
    )
    .context("00Build kernel install failed")?;

    check_kernel_installed_via_recipe(repo_root, output_dir, spec)
        .context("00Build kernel check failed after install")?;
    Ok(S00BuildKernelEnsureOutcome::InstalledNow)
}

fn build_dir_for_output_dir(repo_root: &Path, output_dir: &Path) -> Result<PathBuf> {
    let output_leaf = output_dir
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not derive distro dir from '{}'",
                output_dir.display()
            )
        })?;
    if output_leaf.is_empty() {
        bail!("Empty distro dir derived from '{}'", output_dir.display());
    }

    let legacy_crate_dir = match output_leaf {
        "levitate" | "leviso" => "leviso",
        "acorn" | "AcornOS" => "AcornOS",
        "iuppiter" | "IuppiterOS" => "IuppiterOS",
        "ralph" | "RalphOS" => "RalphOS",
        other => {
            bail!(
                "Unsupported distro output dir '{}' for kernel recipe build dir resolution",
                other
            )
        }
    };

    Ok(repo_root.join(legacy_crate_dir).join("downloads"))
}

fn kernel_defines(spec: &S00BuildKernelSpec) -> Vec<(&str, &str)> {
    vec![
        ("KERNEL_VERSION", spec.kernel_version.as_str()),
        ("KERNEL_SHA256", spec.kernel_sha256.as_str()),
        ("KERNEL_LOCALVERSION", spec.kernel_localversion.as_str()),
        ("KERNEL_FORCE_REBUILD", "0"),
        ("MODULE_INSTALL_PATH", spec.module_install_path.as_str()),
    ]
}

/// Run 00Build evidence script and require success marker.
pub fn run_00build_evidence_script(
    repo_root: &Path,
    variant_dir: &Path,
    output_dir: &Path,
    spec: &S00BuildEvidenceSpec,
) -> Result<()> {
    let script = variant_dir.join(&spec.script_path);
    if !script.is_file() {
        bail!("00Build evidence script not found: {}", script.display());
    }

    let kernel_release_path = output_dir.join(&spec.kernel_release_path);
    let kernel_image_path = output_dir.join(&spec.kernel_image_path);
    let iso_path = output_dir.join(&spec.iso_filename);

    let output = Command::new("sh")
        .arg(&script)
        .current_dir(repo_root)
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .output()
        .with_context(|| format!("executing 00Build evidence script '{}'", script.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if !output.status.success() {
        bail!(
            "00Build evidence script failed (status {}): {}",
            output.status,
            combined.trim()
        );
    }

    if !combined.contains(&spec.pass_marker) {
        bail!(
            "00Build evidence script did not emit required pass marker '{}'",
            spec.pass_marker
        );
    }

    Ok(())
}
