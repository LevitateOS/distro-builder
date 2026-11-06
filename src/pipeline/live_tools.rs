use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use distro_contract::RuntimeActionContract;

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
    distro_id: &str,
    runtime_actions: &[RuntimeActionContract],
) -> Result<()> {
    let tool_payload_dest_dirs =
        tool_payload_destination_dirs(rootfs_source_dir, tool_payload_dir)?;

    for action in runtime_actions {
        match action {
            RuntimeActionContract::ToolPayloadWorkspaceBinary {
                package,
                binary,
                target,
            } => install_tool_payload_workspace_binary(
                repo_root,
                &tool_payload_dest_dirs,
                package,
                binary.as_deref(),
                target.as_deref(),
            )
            .with_context(|| {
                format!(
                    "installing tool payload workspace binary '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    distro_id
                )
            })?,
            RuntimeActionContract::RootfsWorkspaceBinary {
                package,
                binary,
                target,
                destination,
            } => install_rootfs_workspace_binary(
                repo_root,
                rootfs_source_dir,
                package,
                binary.as_deref(),
                target.as_deref(),
                Path::new(destination),
            )
            .with_context(|| {
                format!(
                    "installing rootfs workspace binary '{}' into '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    destination,
                    distro_id
                )
            })?,
            RuntimeActionContract::ApkPackages { packages } => {
                ensure_apk_packages(rootfs_source_dir, packages).with_context(|| {
                    format!(
                        "installing live-tools apk packages for '{}': {}",
                        distro_id,
                        packages.join(", ")
                    )
                })?
            }
        }
    }

    Ok(())
}

fn tool_payload_destination_dirs(
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
) -> Result<Vec<PathBuf>> {
    // Preserve merged-/usr roots (e.g. /bin -> usr/bin): creating a real
    // overlay /bin directory would shadow the symlink and hide /bin/bash,/bin/sh.
    let mut dest_dirs = vec![tool_payload_dir.join("usr/bin")];
    let rootfs_bin = rootfs_source_dir.join("bin");
    if let Ok(meta) = fs::symlink_metadata(&rootfs_bin) {
        if meta.file_type().is_dir() {
            dest_dirs.push(tool_payload_dir.join("bin"));
        }
    }
    for dest_dir in &dest_dirs {
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("creating tool destination '{}'", dest_dir.display()))?;
    }
    Ok(dest_dirs)
}

fn install_tool_payload_workspace_binary(
    repo_root: &Path,
    destination_dirs: &[PathBuf],
    package_name: &str,
    binary_name: Option<&str>,
    target: Option<&str>,
) -> Result<()> {
    let built =
        crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
            .with_context(|| {
                format!(
                    "building workspace binary '{}' from package '{}'",
                    binary_name.unwrap_or(package_name),
                    package_name
                )
            })?;
    let output_name = binary_name.unwrap_or(package_name);

    for destination_dir in destination_dirs {
        copy_executable(&built, &destination_dir.join(output_name)).with_context(|| {
            format!(
                "copying tool payload binary '{}' into '{}'",
                built.display(),
                destination_dir.display()
            )
        })?;
    }

    Ok(())
}

fn install_rootfs_workspace_binary(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    package_name: &str,
    binary_name: Option<&str>,
    target: Option<&str>,
    destination: &Path,
) -> Result<()> {
    let built =
        crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
            .with_context(|| {
                format!(
                    "building workspace binary '{}' from package '{}'",
                    binary_name.unwrap_or(package_name),
                    package_name
                )
            })?;
    let destination = rootfs_source_dir.join(destination);
    copy_executable(&built, &destination).with_context(|| {
        format!(
            "copying rootfs workspace binary '{}' -> '{}'",
            built.display(),
            destination.display()
        )
    })
}

pub(crate) fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating destination dir '{}'", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "copying executable from '{}' to '{}'",
            source.display(),
            destination.display()
        )
    })?;
    let mut perms = fs::metadata(destination)
        .with_context(|| format!("reading metadata for '{}'", destination.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(destination, perms).with_context(|| {
        format!(
            "setting executable permissions on '{}'",
            destination.display()
        )
    })?;
    Ok(())
}

fn ensure_apk_packages(rootfs_source_dir: &Path, packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let resolv_conf = rootfs_source_dir.join("etc/resolv.conf");
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
        .arg(rootfs_source_dir)
        .arg(format!(
            "apk add --no-cache --no-check-certificate {}",
            pkg_string
        ))
        .output()
        .with_context(|| {
            format!(
                "running apk package install in rootfs '{}'",
                rootfs_source_dir.display()
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "apk package install failed for packages [{}]: {}\n{}",
        pkg_string,
        stdout.trim(),
        stderr.trim()
    )
}

pub(crate) fn install_shell_color_profile(rootfs_source_dir: &Path) -> Result<()> {
    let profile_path = rootfs_source_dir.join("etc/profile.d/25-shell-color.sh");
    let profile = crate::guest::SHELL_COLOR_PROFILE;
    write_text(&profile_path, profile)
        .with_context(|| format!("writing shell color profile '{}'", profile_path.display()))
}

pub(crate) fn write_executable(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    let mut perms = fs::metadata(path)
        .with_context(|| format!("reading metadata '{}'", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("setting executable permissions on '{}'", path.display()))?;
    Ok(())
}

pub(crate) fn write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    Ok(())
}
