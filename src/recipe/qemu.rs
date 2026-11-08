//! Explicit managed QEMU/OVMF producer for host boot scenarios.
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Install the QEMU/OVMF tools and select them; returns the selection consumers read
/// (`.artifacts/tools/current`).
pub fn prepare_qemu_tools(repo: &Path) -> Result<PathBuf> {
    let recipe = repo.join("distro/builder/recipes/qemu-tools.rhai");
    let tools = super::Invocation::new(repo, "install", recipe, repo.join(".artifacts/work/qemu-tools"))
        .network_host("dl.rockylinux.org:443")
        .run()?
        .directory("tools")?;
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
    let selection = repo.join(".artifacts/tools/current");
    super::outputs::select_directory(&tools, &selection)?;
    Ok(selection)
}
