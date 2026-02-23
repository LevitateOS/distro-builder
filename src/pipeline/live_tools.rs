use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage02InstallExperience {
    Ux,
    AutomatedSsh,
}

impl Stage02InstallExperience {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ux => "ux",
            Self::AutomatedSsh => "automated_ssh",
        }
    }
}

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_experience: Stage02InstallExperience,
) -> Result<()> {
    let target = match distro_id {
        "levitate" | "ralph" => None,
        "acorn" | "iuppiter" => Some("x86_64-unknown-linux-musl"),
        other => bail!("unsupported distro id for tool wiring: '{}'", other),
    };

    if matches!(distro_id, "acorn" | "iuppiter") {
        ensure_musl_packages(rootfs_source_dir, distro_id, install_experience)
            .with_context(|| format!("installing musl package additions for '{}'", distro_id))?;
    }
    install_mode_payload(rootfs_source_dir, distro_id, install_experience).with_context(|| {
        format!(
            "writing Stage 02 install experience payload for '{}'",
            distro_id
        )
    })?;

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

fn ensure_musl_packages(
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_experience: Stage02InstallExperience,
) -> Result<()> {
    let mut packages: Vec<&str> = match distro_id {
        "acorn" => vec!["curl", "pciutils", "smartmontools", "hdparm", "vim", "htop"],
        "iuppiter" => vec!["smartmontools", "hdparm", "sg3_utils"],
        _ => Vec::new(),
    };
    if install_experience == Stage02InstallExperience::Ux {
        packages.push("tmux");
    }

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

fn install_mode_payload(
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_experience: Stage02InstallExperience,
) -> Result<()> {
    let marker_dir = rootfs_source_dir.join("usr/lib/levitate/stage-02");
    fs::create_dir_all(&marker_dir).with_context(|| {
        format!(
            "creating Stage 02 install experience marker dir '{}'",
            marker_dir.display()
        )
    })?;
    let marker_path = marker_dir.join("install-experience");
    fs::write(&marker_path, format!("{}\n", install_experience.as_str())).with_context(|| {
        format!(
            "writing Stage 02 install experience marker '{}'",
            marker_path.display()
        )
    })?;

    let entrypoint_path = rootfs_source_dir.join("usr/local/bin/stage-02-install-entrypoint");
    let entrypoint_script = match install_experience {
        Stage02InstallExperience::Ux => format!(
            "#!/bin/sh\n\
set -eu\n\
\n\
echo \"[{distro}] Stage 02 install experience: UX\"\n\
echo \"Starting local interactive install helper if available...\"\n\
\n\
if command -v levitate-install-docs-split >/dev/null 2>&1; then\n\
    exec levitate-install-docs-split\n\
fi\n\
if command -v levitate-install-docs >/dev/null 2>&1; then\n\
    exec levitate-install-docs\n\
fi\n\
if command -v acorn-docs >/dev/null 2>&1; then\n\
    exec acorn-docs\n\
fi\n\
\n\
echo \"No local docs TUI binary found; dropping to shell.\"\n\
exec \"${{SHELL:-/bin/sh}}\" -l\n",
            distro = distro_id
        ),
        Stage02InstallExperience::AutomatedSsh => format!(
            "#!/bin/sh\n\
set -eu\n\
\n\
echo \"[{distro}] Stage 02 install experience: automated SSH\"\n\
echo \"This ISO profile is intended for SSH-driven automation (qcow2/.img pipelines).\"\n\
exec \"${{SHELL:-/bin/sh}}\" -l\n",
            distro = distro_id
        ),
    };
    write_executable(&entrypoint_path, &entrypoint_script).with_context(|| {
        format!(
            "installing Stage 02 entrypoint script '{}'",
            entrypoint_path.display()
        )
    })?;

    if install_experience == Stage02InstallExperience::Ux {
        let ux_profile_path = rootfs_source_dir.join("etc/profile.d/30-stage-02-install-ux.sh");
        let ux_profile = "#!/bin/sh\n\
case \"$-\" in\n\
    *i*) ;;\n\
    *) return 0 ;;\n\
esac\n\
\n\
[ -n \"${TMUX:-}\" ] && return 0\n\
[ \"${STAGE02_UX_LAUNCHED:-0}\" = \"1\" ] && return 0\n\
\n\
TTY=\"$(tty 2>/dev/null || true)\"\n\
[ \"$TTY\" = \"/dev/tty1\" ] || return 0\n\
\n\
export STAGE02_UX_LAUNCHED=1\n\
exec /usr/local/bin/stage-02-install-entrypoint\n";
        write_text(&ux_profile_path, ux_profile).with_context(|| {
            format!(
                "installing Stage 02 UX profile hook '{}'",
                ux_profile_path.display()
            )
        })?;
    }

    Ok(())
}

fn write_executable(path: &Path, content: &str) -> Result<()> {
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

fn write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    Ok(())
}
