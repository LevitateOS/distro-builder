use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
) -> Result<()> {
    let target = match distro_id {
        "levitate" | "ralph" => None,
        "acorn" | "iuppiter" => Some("x86_64-unknown-linux-musl"),
        other => bail!("unsupported distro id for tool wiring: '{}'", other),
    };

    if matches!(distro_id, "acorn" | "iuppiter") {
        ensure_musl_packages(rootfs_source_dir, distro_id)
            .with_context(|| format!("installing musl package additions for '{}'", distro_id))?;
    }

    let dest_dirs = [
        rootfs_source_dir.join("usr/bin"),
        rootfs_source_dir.join("bin"),
    ];
    for dest_dir in &dest_dirs {
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("creating tool destination '{}'", dest_dir.display()))?;
    }

    for tool in ["recstrap", "recfstab", "recchroot"] {
        let built = build_workspace_binary(repo_root, tool, target)
            .with_context(|| format!("building workspace tool '{}'", tool))?;
        if !built.is_file() {
            bail!(
                "expected built tool binary not found: '{}'",
                built.display()
            );
        }
        for dest_dir in &dest_dirs {
            let target = dest_dir.join(tool);
            fs::copy(&built, &target).with_context(|| {
                format!(
                    "copying tool '{}' -> '{}'",
                    built.display(),
                    target.display()
                )
            })?;
            let mut perms = fs::metadata(&target)
                .with_context(|| format!("reading permissions for '{}'", target.display()))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&target, perms).with_context(|| {
                format!("setting executable permissions on '{}'", target.display())
            })?;
        }
    }

    Ok(())
}

fn ensure_musl_packages(rootfs_source_dir: &Path, distro_id: &str) -> Result<()> {
    let packages: &[&str] = match distro_id {
        "acorn" => &["curl", "pciutils", "smartmontools", "hdparm", "vim", "htop"],
        "iuppiter" => &["smartmontools", "hdparm", "sg3_utils"],
        _ => &[],
    };

    if packages.is_empty() {
        return Ok(());
    }

    let resolv_conf = rootfs_source_dir.join("etc/resolv.conf");
    if let Some(parent) = resolv_conf.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::copy("/etc/resolv.conf", &resolv_conf).with_context(|| {
        format!(
            "copying host resolv.conf into stage rootfs '{}'",
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
        "apk package install failed for '{}': {}\n{}",
        distro_id,
        stdout.trim(),
        stderr.trim()
    )
}

fn build_workspace_binary(
    repo_root: &Path,
    package_name: &str,
    target: Option<&str>,
) -> Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build").arg("-q").arg("-p").arg(package_name);
    if let Some(target_triple) = target {
        cmd.arg("--target").arg(target_triple);
    }

    let output = cmd.current_dir(repo_root).output().with_context(|| {
        format!(
            "running cargo build for package '{}' at '{}'",
            package_name,
            repo_root.display()
        )
    })?;

    if output.status.success() {
        let built = match target {
            Some(target_triple) => repo_root
                .join("target")
                .join(target_triple)
                .join("debug")
                .join(package_name),
            None => repo_root.join("target/debug").join(package_name),
        };
        return Ok(built);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "cargo build failed for package '{}': {}\n{}",
        package_name,
        stdout.trim(),
        stderr.trim()
    )
}
