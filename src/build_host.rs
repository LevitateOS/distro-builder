use anyhow::Result;
use std::path::Path;

pub use crate::pipeline::kernel::{
    EvidenceSpec as BuildHostEvidenceSpec, KernelEnsureOutcome as BuildHostKernelEnsureOutcome,
    KernelSpec as BuildHostKernelSpec,
};

pub fn check_kernel_preinstalled_via_recipe(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &BuildHostKernelSpec,
) -> Result<()> {
    crate::pipeline::kernel::check_kernel_installed_with_recipe(
        repo_root,
        variant_dir,
        distro_id,
        kernel_output_dir,
        spec,
    )
}

pub fn ensure_kernel_preinstalled_via_recipe(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    kernel_output_dir: &Path,
    spec: &BuildHostKernelSpec,
) -> Result<BuildHostKernelEnsureOutcome> {
    crate::pipeline::kernel::ensure_kernel_preinstalled_with_recipe(
        repo_root,
        variant_dir,
        distro_id,
        kernel_output_dir,
        spec,
    )
}

pub fn run_build_host_evidence_script(
    repo_root: &Path,
    variant_dir: &Path,
    kernel_output_dir: &Path,
    release_output_dir: &Path,
    spec: &BuildHostEvidenceSpec,
) -> Result<()> {
    crate::pipeline::kernel::run_build_evidence_script(
        repo_root,
        variant_dir,
        kernel_output_dir,
        release_output_dir,
        spec,
    )
}
