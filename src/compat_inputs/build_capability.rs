use anyhow::Result;
use std::path::Path;

pub use crate::pipeline::kernel::{
    EvidenceSpec as S00BuildEvidenceSpec, KernelEnsureOutcome as S00BuildKernelEnsureOutcome,
    KernelSpec as S00BuildKernelSpec,
};

pub fn check_kernel_installed_via_recipe(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &S00BuildKernelSpec,
) -> Result<()> {
    crate::pipeline::kernel::check_kernel_installed_with_recipe(
        repo_root,
        variant_dir,
        distro_id,
        kernel_output_dir,
        spec,
    )
}

pub fn ensure_kernel_installed_via_recipe(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &S00BuildKernelSpec,
) -> Result<S00BuildKernelEnsureOutcome> {
    crate::pipeline::kernel::ensure_kernel_preinstalled_with_recipe(
        repo_root,
        variant_dir,
        distro_id,
        kernel_output_dir,
        spec,
    )
}

pub fn run_00build_evidence_script(
    repo_root: &Path,
    variant_dir: &Path,
    kernel_output_dir: &Path,
    stage_output_dir: &Path,
    spec: &S00BuildEvidenceSpec,
) -> Result<()> {
    crate::pipeline::kernel::run_build_evidence_script(
        repo_root,
        variant_dir,
        kernel_output_dir,
        stage_output_dir,
        spec,
    )
}
