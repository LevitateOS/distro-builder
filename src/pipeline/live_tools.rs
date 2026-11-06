//! The variant's live-tools runtime actions (`runtime_profiles` in products.toml).

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::artifact::filesystem::copy_executable;
use distro_contract::RuntimeActionContract;

/// Runs the actions. `rootfs` and `overlay` are the live-tools product's two layers:
/// tool payload binaries go into the overlay, rootfs binaries and apk packages into
/// the rootfs (which is what `recstrap` installs).
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
            RuntimeActionContract::ApkPackages { packages } => {
                ensure_apk_packages(rootfs, packages).with_context(|| {
                    format!(
                        "installing live-tools apk packages for '{distro_id}': {}",
                        packages.join(", ")
                    )
                })?
            }
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

fn ensure_apk_packages(rootfs: &Path, packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    // apk resolves its mirror through the host's DNS while it runs; the rootfs keeps
    // its own resolv.conf (or none) afterwards.
    let resolv_conf = rootfs.join("etc/resolv.conf");
    let original = fs::read(&resolv_conf).ok();
    if let Some(parent) = resolv_conf.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::copy("/etc/resolv.conf", &resolv_conf).with_context(|| {
        format!(
            "copying host resolv.conf into prepared rootfs '{}'",
            resolv_conf.display()
        )
    })?;

    let pkg_string = packages.join(" ");
    let output = Command::new("unshare")
        .arg("-Urpf")
        .arg("/bin/sh")
        .arg("-c")
        .arg("chroot \"$1\" /bin/sh -lc \"$2\"")
        .arg("_")
        .arg(rootfs)
        .arg(format!(
            "apk add --no-cache --no-check-certificate {}",
            pkg_string
        ))
        .output()
        .with_context(|| {
            format!(
                "running apk package install in rootfs '{}'",
                rootfs.display()
            )
        });

    match original {
        Some(content) => fs::write(&resolv_conf, content),
        None => fs::remove_file(&resolv_conf),
    }
    .with_context(|| format!("restoring '{}'", resolv_conf.display()))?;

    let output = output?;
    if output.status.success() {
        return Ok(());
    }
    bail!(
        "apk package install failed for packages [{}]: {}\n{}",
        pkg_string,
        String::from_utf8_lossy(&output.stdout).trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}
