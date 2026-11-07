//! The variant's live-tools runtime actions (`runtime_profiles` in products.toml).

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::artifact::filesystem::copy_executable;
use distro_contract::RuntimeActionContract;

/// Runs the actions. `rootfs` and `overlay` are the live-tools product's two layers:
/// tool payload binaries go into the overlay, rootfs binaries into the rootfs (which
/// is what `recstrap` installs). Packages come from the rootfs source recipe, never
/// from here.
pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs: &Path,
    overlay: &Path,
    distro_id: &str,
    runtime_actions: &[RuntimeActionContract],
) -> Result<()> {
    for action in runtime_actions {
        match action {
            RuntimeActionContract::ToolPayloadWorkspaceBinary {
                package,
                binary,
                target,
            } => {
                let name = binary.as_deref().unwrap_or(package);
                let destinations = overlay_bin_dirs(rootfs, overlay)
                    .into_iter()
                    .map(|dir| dir.join(name))
                    .collect::<Vec<_>>();
                build_and_copy(
                    repo_root,
                    package,
                    binary.as_deref(),
                    target.as_deref(),
                    &destinations,
                )
                .with_context(|| {
                    format!("installing '{name}' into the overlay for '{distro_id}'")
                })?
            }
            RuntimeActionContract::RootfsWorkspaceBinary {
                package,
                binary,
                target,
                destination,
            } => build_and_copy(
                repo_root,
                package,
                binary.as_deref(),
                target.as_deref(),
                &[rootfs.join(destination)],
            )
            .with_context(|| {
                format!(
                    "installing '{}' into the rootfs at '{destination}' for '{distro_id}'",
                    binary.as_deref().unwrap_or(package)
                )
            })?,
        }
    }
    Ok(())
}

/// The overlay's `usr/bin`, plus its `bin` when the rootfs's `/bin` is a real
/// directory. A merged-/usr rootfs (`/bin -> usr/bin`) gets no overlay `/bin`: a real
/// one would shadow the symlink and hide `/bin/sh`.
fn overlay_bin_dirs(rootfs: &Path, overlay: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![overlay.join("usr/bin")];
    if fs::symlink_metadata(rootfs.join("bin")).is_ok_and(|meta| meta.file_type().is_dir()) {
        dirs.push(overlay.join("bin"));
    }
    dirs
}

/// Builds the package's binary through its recipe and copies it to each destination.
fn build_and_copy(
    repo_root: &Path,
    package: &str,
    binary: Option<&str>,
    target: Option<&str>,
    destinations: &[PathBuf],
) -> Result<()> {
    let built = crate::recipe::tools::build_tool_program(repo_root, package, binary, target)
        .with_context(|| {
            format!(
                "building '{}' from package '{package}'",
                binary.unwrap_or(package)
            )
        })?;
    for destination in destinations {
        copy_executable(&built, destination)?;
    }
    Ok(())
}
