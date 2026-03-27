use anyhow::{bail, Context, Result};
use distro_contract::VariantOwnerPaths;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::pipeline::paths::normalize_distro_id;

#[derive(Debug, Clone)]
pub struct KernelSpec {
    pub recipe_kernel_script: String,
    pub kernel_kconfig_path: String,
}

#[derive(Debug, Clone)]
pub struct EvidenceSpec {
    pub script_path: String,
    pub pass_marker: String,
    pub kernel_release_path: String,
    pub kernel_image_path: String,
    pub iso_filename: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KernelEnsureOutcome {
    AlreadyInstalled,
}

pub fn check_kernel_installed_with_recipe(
    repo_root: &Path,
    variant_paths: &VariantOwnerPaths,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &KernelSpec,
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
    let build_dir = work_dir_for_distro(repo_root, distro_id)?;
    let kernel_kconfig_path = variant_paths
        .build_host_declared_path(&spec.kernel_kconfig_path)
        .to_string_lossy()
        .to_string();

    let recipe_bin = crate::recipe::find_recipe(repo_root)
        .context("Resolving recipe binary for build kernel")?;
    let kernel_artifact_root = kernel_output_dir.to_string_lossy().to_string();
    let defines = kernel_recipe_defines(&kernel_kconfig_path, &kernel_artifact_root);
    crate::recipe::run_recipe_phase_json_with_defines(
        &recipe_bin.path,
        "isinstalled",
        &recipe_script,
        &build_dir,
        &defines,
        Some(&recipes_path),
    )
    .context("build kernel isinstalled check failed")?;

    Ok(())
}

pub fn ensure_kernel_preinstalled_with_recipe(
    repo_root: &Path,
    variant_paths: &VariantOwnerPaths,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &KernelSpec,
) -> Result<KernelEnsureOutcome> {
    match check_kernel_installed_with_recipe(
        repo_root,
        variant_paths,
        distro_id,
        kernel_output_dir,
        spec,
    ) {
        Ok(()) => Ok(KernelEnsureOutcome::AlreadyInstalled),
        Err(e) => bail!(
            "build-host kernel is not preinstalled for '{}': {}\n\
             Kernel rebuilds are forbidden during stage ISO builds.\n\
             Remediation: run 'cargo xtask kernels build {}' (or '--rebuild' if provenance is stale), then retry the ISO build.",
            distro_id,
            e,
            distro_id
        ),
    }
}

pub fn run_build_evidence_script(
    repo_root: &Path,
    variant_paths: &VariantOwnerPaths,
    kernel_output_dir: &Path,
    stage_output_dir: &Path,
    spec: &EvidenceSpec,
) -> Result<()> {
    let script = variant_paths.build_host_declared_path(&spec.script_path);
    if !script.is_file() {
        bail!("build evidence script not found: {}", script.display());
    }

    let kernel_release_path = kernel_output_dir.join(&spec.kernel_release_path);
    let kernel_image_path = kernel_output_dir.join(&spec.kernel_image_path);
    let iso_path = stage_output_dir.join(&spec.iso_filename);

    let output = Command::new("sh")
        .arg(&script)
        .current_dir(repo_root)
        .env("KERNEL_RELEASE_PATH", &kernel_release_path)
        .env("KERNEL_IMAGE_PATH", &kernel_image_path)
        .env("ISO_PATH", &iso_path)
        .output()
        .with_context(|| format!("executing build evidence script '{}'", script.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);

    if !output.status.success() {
        bail!(
            "build evidence script failed (status {}): {}",
            output.status,
            combined.trim()
        );
    }

    if !combined.contains(&spec.pass_marker) {
        bail!(
            "build evidence script did not emit required pass marker '{}'",
            spec.pass_marker
        );
    }

    Ok(())
}

fn work_dir_for_distro(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = normalize_distro_id(distro_id, "kernel recipe build directory")?;
    let build_dir = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads");
    std::fs::create_dir_all(&build_dir).with_context(|| {
        format!(
            "creating kernel recipe work directory '{}'",
            build_dir.display()
        )
    })?;
    Ok(build_dir)
}

fn kernel_recipe_defines<'a>(
    kernel_kconfig_path: &'a str,
    kernel_artifact_root: &'a str,
) -> Vec<(&'a str, &'a str)> {
    vec![
        ("KERNEL_KCONFIG_PATH", kernel_kconfig_path),
        ("KERNEL_ARTIFACT_ROOT", kernel_artifact_root),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn kernel_recipe_defines_exclude_kernel_source_metadata() {
        let defines = kernel_recipe_defines(
            "distro-variants/levitate/build-host/kernel/kconfig",
            "/tmp/kernel",
        );
        assert_eq!(
            defines,
            vec![
                (
                    "KERNEL_KCONFIG_PATH",
                    "distro-variants/levitate/build-host/kernel/kconfig"
                ),
                ("KERNEL_ARTIFACT_ROOT", "/tmp/kernel"),
            ]
        );
    }

    #[test]
    fn work_dir_for_distro_normalizes_legacy_aliases() {
        let unique = format!(
            "levitateos-kernel-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("create temp repo root");

        let work_dir = work_dir_for_distro(&repo_root, "leviso").expect("resolve alias work dir");
        assert!(
            work_dir.ends_with(".artifacts/work/levitate/downloads"),
            "expected normalized levitate kernel work dir, got {}",
            work_dir.display()
        );
    }
}
