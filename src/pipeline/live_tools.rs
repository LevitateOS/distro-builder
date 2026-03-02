use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::copy_dir_recursive;

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
    tool_payload_dir: &Path,
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
    if distro_id == "iuppiter" {
        install_iuppiter_runtime_payload(repo_root, rootfs_source_dir, target).with_context(
            || {
                format!(
                    "installing iuppiter runtime payload into Stage 02 rootfs for '{}'",
                    distro_id
                )
            },
        )?;
    }
    install_mode_payload(tool_payload_dir, distro_id, install_experience).with_context(|| {
        format!(
            "writing Stage 02 install experience payload for '{}'",
            distro_id
        )
    })?;
    if install_experience == Stage02InstallExperience::Ux {
        install_split_launcher(repo_root, tool_payload_dir, target).with_context(|| {
            format!(
                "installing Stage 02 split launcher binary for '{}'",
                distro_id
            )
        })?;
    }

    let dest_dirs = [
        tool_payload_dir.join("usr/bin"),
        tool_payload_dir.join("bin"),
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

fn install_iuppiter_runtime_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    target: Option<&str>,
) -> Result<()> {
    let recab = build_workspace_binary(repo_root, "recab", target)
        .context("building recab binary for iuppiter Stage 02 rootfs payload")?;
    if !recab.is_file() {
        bail!(
            "expected built recab binary not found at '{}'",
            recab.display()
        );
    }
    let recab_dst = rootfs_source_dir.join("usr/bin/recab");
    copy_executable(&recab, &recab_dst).with_context(|| {
        format!(
            "copying recab binary '{}' -> '{}'",
            recab.display(),
            recab_dst.display()
        )
    })?;

    let dar_root = resolve_iuppiter_dar_root(repo_root)?;
    let dar_bin = resolve_iuppiter_dar_bin(&dar_root, target)?;
    let dar_spa = resolve_iuppiter_dar_spa_dir(&dar_root)?;

    let dar_dst = rootfs_source_dir.join("opt/iuppiter/iuppiter-dar");
    copy_executable(&dar_bin, &dar_dst).with_context(|| {
        format!(
            "copying iuppiter-dar binary '{}' -> '{}'",
            dar_bin.display(),
            dar_dst.display()
        )
    })?;

    ensure_symlink(
        Path::new("/opt/iuppiter/iuppiter-dar"),
        &rootfs_source_dir.join("usr/bin/iuppiter-dar"),
    )
    .context("linking iuppiter-dar into /usr/bin for PATH discovery")?;

    let spa_dst = rootfs_source_dir.join("usr/share/iuppiter/spa");
    if spa_dst.exists() {
        fs::remove_dir_all(&spa_dst).with_context(|| {
            format!(
                "removing previous iuppiter SPA directory '{}'",
                spa_dst.display()
            )
        })?;
    }
    copy_dir_recursive(&dar_spa, &spa_dst).with_context(|| {
        format!(
            "copying iuppiter-dar SPA '{}' -> '{}'",
            dar_spa.display(),
            spa_dst.display()
        )
    })?;

    Ok(())
}

fn resolve_iuppiter_dar_root(repo_root: &Path) -> Result<PathBuf> {
    if let Ok(path) = env::var("IUPPITER_DAR_ROOT") {
        let root = PathBuf::from(path);
        if root.is_dir() {
            return Ok(root);
        }
        bail!(
            "IUPPITER_DAR_ROOT is set but not a directory: '{}'",
            root.display()
        );
    }

    let mut candidates = vec![repo_root.join("iuppiter-dar")];
    if let Some(parent) = repo_root.parent() {
        candidates.push(parent.join("iuppiter-dar"));
    }
    if let Ok(home) = env::var("HOME") {
        candidates.push(PathBuf::from(home).join("iuppiter-dar"));
    }

    for candidate in candidates {
        if candidate.is_dir() {
            return Ok(candidate);
        }
    }

    bail!(
        "iuppiter-dar root not found. Set IUPPITER_DAR_ROOT to a checkout containing \
target/*/release/iuppiter-daemon and dist/ (for example: export IUPPITER_DAR_ROOT=\"$HOME/iuppiter-dar\")."
    );
}

fn resolve_iuppiter_dar_bin(dar_root: &Path, target: Option<&str>) -> Result<PathBuf> {
    if let Ok(path) = env::var("IUPPITER_DAR_BIN") {
        let bin = PathBuf::from(path);
        if bin.is_file() {
            return Ok(bin);
        }
        bail!(
            "IUPPITER_DAR_BIN is set but not a file: '{}'",
            bin.display()
        );
    }

    let mut candidates = Vec::new();
    if let Some(target_triple) = target {
        candidates.push(
            dar_root
                .join("target")
                .join(target_triple)
                .join("release")
                .join("iuppiter-daemon"),
        );
    }
    candidates.push(dar_root.join("target/release/iuppiter-daemon"));
    candidates.push(dar_root.join("target/release/iuppiter-dar"));

    for candidate in &candidates {
        if candidate.is_file() {
            return Ok(candidate.clone());
        }
    }

    let target_hint = target.unwrap_or("x86_64-unknown-linux-musl");
    bail!(
        "iuppiter-dar binary missing under '{}'. Build it first, then rerun Stage 02 build.\n\
Expected one of: {}\n\
Suggested command:\n\
  cd '{}' && cargo build --target {} --release",
        dar_root.display(),
        candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        dar_root.display(),
        target_hint
    );
}

fn resolve_iuppiter_dar_spa_dir(dar_root: &Path) -> Result<PathBuf> {
    if let Ok(path) = env::var("IUPPITER_DAR_SPA_DIR") {
        let spa = PathBuf::from(path);
        if spa.is_dir() {
            return Ok(spa);
        }
        bail!(
            "IUPPITER_DAR_SPA_DIR is set but not a directory: '{}'",
            spa.display()
        );
    }

    let spa = dar_root.join("dist");
    if !spa.is_dir() {
        bail!(
            "iuppiter-dar SPA directory missing: '{}'. Build SPA first (for example: `cd {} && bun run build`).",
            spa.display(),
            dar_root.display()
        );
    }
    Ok(spa)
}

fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
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

fn ensure_symlink(link_target: &Path, link_path: &Path) -> Result<()> {
    if let Some(parent) = link_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating symlink parent '{}'", parent.display()))?;
    }

    match fs::symlink_metadata(link_path) {
        Ok(metadata) => {
            if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(link_path).with_context(|| {
                    format!(
                        "removing existing directory before symlink '{}'",
                        link_path.display()
                    )
                })?;
            } else {
                fs::remove_file(link_path).with_context(|| {
                    format!(
                        "removing existing file before symlink '{}'",
                        link_path.display()
                    )
                })?;
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err).with_context(|| {
                format!(
                    "reading existing symlink metadata for '{}'",
                    link_path.display()
                )
            });
        }
    }

    symlink(link_target, link_path).with_context(|| {
        format!(
            "creating symlink '{}' -> '{}'",
            link_path.display(),
            link_target.display()
        )
    })?;
    Ok(())
}

fn ensure_musl_packages(
    rootfs_source_dir: &Path,
    distro_id: &str,
    _install_experience: Stage02InstallExperience,
) -> Result<()> {
    let packages: Vec<&str> = match distro_id {
        "acorn" => vec!["curl", "pciutils", "smartmontools", "hdparm", "vim", "htop"],
        "iuppiter" => vec!["smartmontools", "hdparm", "sg3_utils"],
        _ => Vec::new(),
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

fn build_workspace_binary_named(
    repo_root: &Path,
    package_name: &str,
    binary_name: &str,
    target: Option<&str>,
) -> Result<PathBuf> {
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("-q")
        .arg("-p")
        .arg(package_name)
        .arg("--bin")
        .arg(binary_name);
    if let Some(target_triple) = target {
        cmd.arg("--target").arg(target_triple);
    }

    let output = cmd.current_dir(repo_root).output().with_context(|| {
        format!(
            "running cargo build for package '{}' (bin '{}') at '{}'",
            package_name,
            binary_name,
            repo_root.display()
        )
    })?;

    if output.status.success() {
        let built = match target {
            Some(target_triple) => repo_root
                .join("target")
                .join(target_triple)
                .join("debug")
                .join(binary_name),
            None => repo_root.join("target/debug").join(binary_name),
        };
        return Ok(built);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "cargo build failed for package '{}' (bin '{}'): {}\n{}",
        package_name,
        binary_name,
        stdout.trim(),
        stderr.trim()
    )
}

fn install_split_launcher(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    target: Option<&str>,
) -> Result<()> {
    let binary_name = "levitate-install-docs-split";
    let built = build_workspace_binary_named(repo_root, "stage02-split-pane", binary_name, target)
        .context("building Stage 02 split-pane launcher")?;
    if !built.is_file() {
        bail!(
            "expected Stage 02 split launcher binary not found at '{}'",
            built.display()
        );
    }

    let dest = rootfs_source_dir.join("usr/local/bin").join(binary_name);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("creating split launcher destination '{}'", parent.display())
        })?;
    }
    fs::copy(&built, &dest).with_context(|| {
        format!(
            "copying Stage 02 split launcher '{}' -> '{}'",
            built.display(),
            dest.display()
        )
    })?;
    let mut perms = fs::metadata(&dest)
        .with_context(|| format!("reading permissions for '{}'", dest.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&dest, perms)
        .with_context(|| format!("setting executable permissions on '{}'", dest.display()))?;
    Ok(())
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
choose_install_helper() {{\n\
    if [ -x /usr/local/bin/levitate-install-docs-split ]; then\n\
        printf '%s\\n' \"/usr/local/bin/levitate-install-docs-split\"\n\
        return 0\n\
    fi\n\
    if command -v levitate-install-docs-split >/dev/null 2>&1; then\n\
        printf '%s\\n' \"levitate-install-docs-split\"\n\
        return 0\n\
    fi\n\
    if command -v levitate-install-docs >/dev/null 2>&1; then\n\
        printf '%s\\n' \"levitate-install-docs\"\n\
        return 0\n\
    fi\n\
    if command -v acorn-docs >/dev/null 2>&1; then\n\
        printf '%s\\n' \"acorn-docs\"\n\
        return 0\n\
    fi\n\
    return 1\n\
}}\n\
\n\
if [ \"${{1:-}}\" = \"--probe\" ]; then\n\
    helper=\"$(choose_install_helper || true)\"\n\
    if [ -n \"$helper\" ]; then\n\
        printf 'stage02-entrypoint-helper=%s\\n' \"$helper\"\n\
        exit 0\n\
    fi\n\
    printf 'stage02-entrypoint-helper=none\\n'\n\
    exit 3\n\
fi\n\
\n\
echo \"[{distro}] Stage 02 install experience: UX\"\n\
echo \"Starting local interactive install helper if available...\"\n\
\n\
helper=\"$(choose_install_helper || true)\"\n\
if [ -n \"$helper\" ]; then\n\
    case \"$helper\" in\n\
        */levitate-install-docs-split|levitate-install-docs-split)\n\
            if [ \"${{STAGE02_ENTRYPOINT_SMOKE:-0}}\" = \"1\" ]; then\n\
                exec \"$helper\" --smoke\n\
            fi\n\
            ;;\n\
    esac\n\
    exec \"$helper\"\n\
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
