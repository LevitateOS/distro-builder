//! Explicit managed QEMU/OVMF producer for host boot scenarios.
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub fn prepare_qemu_tools(repo: &Path) -> Result<PathBuf> {
    let recipe = repo.join("distro-builder/recipes/qemu-tools.rhai");
    let binary = super::find_recipe(repo)?;
    let result = super::run_recipe_phase_json_with_inputs(
        &binary.path,
        "install",
        &recipe,
        &repo.join(".artifacts/work/qemu-tools"),
        &[],
        &[],
        &["dl.rockylinux.org:443".into()],
        recipe.parent(),
    )?;
    let tools = super::outputs::directory(&result, "tools")?;
    for file in [
        "usr/bin/qemu-img",
        "usr/libexec/qemu-kvm",
        "usr/share/edk2/ovmf/OVMF_CODE.fd",
        "usr/share/edk2/ovmf/OVMF_VARS.fd",
    ] {
        if !tools.join(file).is_file() {
            bail!("published QEMU tools are missing {file}");
        }
    }
    super::outputs::select_directory(&tools, &repo.join(".artifacts/tools/current"))?;
    Ok(tools)
}
