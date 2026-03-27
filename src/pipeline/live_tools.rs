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
pub(crate) enum InstallExperience {
    Ux,
    AutomatedSsh,
}

impl InstallExperience {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Ux => "ux",
            Self::AutomatedSsh => "automated_ssh",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallDocsFrontend {
    PlainText,
    BunBundle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveToolsRuntimeAction {
    ToolPayloadWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
    },
    RootfsWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
        destination: PathBuf,
    },
    ApkPackages {
        packages: Vec<String>,
    },
    IuppiterDarPayload {
        target: Option<String>,
    },
    InstallModePayload {
        interactive_shell: String,
        ux_docs_frontend: InstallDocsFrontend,
    },
}

const CANONICAL_INSTALL_EXPERIENCE_MARKER: &str = "usr/lib/levitate/install-experience";
const CANONICAL_INSTALL_DOCS_TEXT: &str = "usr/local/share/levitate/install-docs.txt";
const CANONICAL_INSTALL_ENTRYPOINT: &str = "usr/local/bin/levitate-install-entrypoint";
const CANONICAL_INSTALL_UX_PROFILE: &str = "etc/profile.d/30-install-ux.sh";

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
    distro_id: &str,
    install_experience: InstallExperience,
    runtime_actions: &[LiveToolsRuntimeAction],
) -> Result<()> {
    let tool_payload_dest_dirs =
        tool_payload_destination_dirs(rootfs_source_dir, tool_payload_dir)?;

    for action in runtime_actions {
        match action {
            LiveToolsRuntimeAction::ToolPayloadWorkspaceBinary {
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
            LiveToolsRuntimeAction::RootfsWorkspaceBinary {
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
                destination,
            )
            .with_context(|| {
                format!(
                    "installing rootfs workspace binary '{}' into '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    destination.display(),
                    distro_id
                )
            })?,
            LiveToolsRuntimeAction::ApkPackages { packages } => {
                ensure_apk_packages(rootfs_source_dir, packages).with_context(|| {
                    format!(
                        "installing live-tools apk packages for '{}': {}",
                        distro_id,
                        packages.join(", ")
                    )
                })?
            }
            LiveToolsRuntimeAction::IuppiterDarPayload { target } => {
                install_iuppiter_dar_payload(repo_root, rootfs_source_dir, target.as_deref())
                    .with_context(|| {
                        format!(
                            "installing iuppiter DAR runtime payload for '{}'",
                            distro_id
                        )
                    })?
            }
            LiveToolsRuntimeAction::InstallModePayload {
                interactive_shell,
                ux_docs_frontend,
            } => install_mode_payload(
                repo_root,
                rootfs_source_dir,
                distro_id,
                install_experience,
                interactive_shell,
                *ux_docs_frontend,
            )
            .with_context(|| format!("writing install experience payload for '{}'", distro_id))?,
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
    let built = build_workspace_binary_artifact(repo_root, package_name, binary_name, target)
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
    let built = build_workspace_binary_artifact(repo_root, package_name, binary_name, target)
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

fn build_workspace_binary_artifact(
    repo_root: &Path,
    package_name: &str,
    binary_name: Option<&str>,
    target: Option<&str>,
) -> Result<PathBuf> {
    let built = match binary_name {
        Some(binary_name) => {
            build_workspace_binary_named(repo_root, package_name, binary_name, target)
        }
        None => build_workspace_binary(repo_root, package_name, target),
    }?;
    if !built.is_file() {
        bail!(
            "expected built workspace binary not found at '{}'",
            built.display()
        );
    }
    Ok(built)
}

fn install_iuppiter_dar_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    target: Option<&str>,
) -> Result<()> {
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
        "iuppiter-dar binary missing under '{}'. Build it first, then rerun live-tools product preparation.\n\
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
        "apk package install failed for packages [{}]: {}\n{}",
        pkg_string,
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

fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|entry| entry.join(name))
        .find(|candidate| candidate.is_file())
}

fn install_bun_docs_tui_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    docs_cmd_path: &Path,
) -> Result<()> {
    let docs_app_dir = repo_root.join("tui/apps/live-tools/install-docs");
    if !docs_app_dir.is_dir() {
        bail!(
            "live install docs app source missing at '{}'. Expected workspace path 'tui/apps/live-tools/install-docs'.",
            docs_app_dir.display()
        );
    }

    let bun_bin = find_executable_in_path("bun").ok_or_else(|| {
        anyhow::anyhow!(
            "bun is required to build the live install docs payload.\n\
             Remediation: install bun and ensure it is in PATH (https://bun.sh)."
        )
    })?;

    // Ensure docs app dependencies are available for bundling.
    let docs_tui_kit = docs_app_dir.join("node_modules/@levitate/tui-kit/package.json");
    if !docs_tui_kit.is_file() {
        let install = Command::new(&bun_bin)
            .arg("install")
            .current_dir(&docs_app_dir)
            .output()
            .with_context(|| {
                format!(
                    "running bun install for live install docs app at '{}'",
                    docs_app_dir.display()
                )
            })?;
        if !install.status.success() {
            let stdout = String::from_utf8_lossy(&install.stdout);
            let stderr = String::from_utf8_lossy(&install.stderr);
            bail!(
                "bun install failed for live install docs app '{}': {}\n{}",
                docs_app_dir.display(),
                stdout.trim(),
                stderr.trim()
            );
        }
    }

    let docs_bundle_dir = rootfs_source_dir.join("usr/local/share/levitate/docs-tui");
    fs::create_dir_all(&docs_bundle_dir).with_context(|| {
        format!(
            "creating live install docs bundle directory '{}'",
            docs_bundle_dir.display()
        )
    })?;
    let docs_bundle = docs_bundle_dir.join("levitate-install-docs.js");

    let build = Command::new(&bun_bin)
        .arg("build")
        .arg("src/main.ts")
        .arg("--target=bun")
        .arg("--minify")
        .arg("--outfile")
        .arg(&docs_bundle)
        .current_dir(&docs_app_dir)
        .output()
        .with_context(|| {
            format!(
                "building live install docs bundle from '{}' to '{}'",
                docs_app_dir.display(),
                docs_bundle.display()
            )
        })?;
    if !build.status.success() {
        let stdout = String::from_utf8_lossy(&build.stdout);
        let stderr = String::from_utf8_lossy(&build.stderr);
        bail!(
            "bun build failed for live install docs bundle '{}': {}\n{}",
            docs_app_dir.display(),
            stdout.trim(),
            stderr.trim()
        );
    }
    if !docs_bundle.is_file() {
        bail!(
            "live install docs bundle missing after build at '{}'",
            docs_bundle.display()
        );
    }

    let bun_dest = rootfs_source_dir.join("usr/local/bin/bun");
    if let Some(parent) = bun_dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating bun destination '{}'", parent.display()))?;
    }
    fs::copy(&bun_bin, &bun_dest).with_context(|| {
        format!(
            "copying bun runtime '{}' -> '{}'",
            bun_bin.display(),
            bun_dest.display()
        )
    })?;
    let mut bun_perms = fs::metadata(&bun_dest)
        .with_context(|| format!("reading permissions for '{}'", bun_dest.display()))?
        .permissions();
    bun_perms.set_mode(0o755);
    fs::set_permissions(&bun_dest, bun_perms)
        .with_context(|| format!("setting executable permissions on '{}'", bun_dest.display()))?;

    let docs_cmd = "#!/bin/sh\n\
set -eu\n\
\n\
APP=\"/usr/local/share/levitate/docs-tui/levitate-install-docs.js\"\n\
if [ ! -r \"$APP\" ]; then\n\
    echo \"install docs bundle missing at $APP\" >&2\n\
    exit 1\n\
fi\n\
\n\
case \"${TERM:-}\" in\n\
    \"\"|dumb|vt100|vt102|linux)\n\
        export TERM=xterm-256color\n\
        ;;\n\
esac\n\
export COLORTERM=\"${COLORTERM:-truecolor}\"\n\
export FORCE_COLOR=\"${FORCE_COLOR:-3}\"\n\
if [ -n \"${NO_COLOR:-}\" ]; then\n\
    unset NO_COLOR\n\
fi\n\
\n\
exec /usr/local/bin/bun \"$APP\" \"$@\"\n";
    write_executable(docs_cmd_path, docs_cmd).with_context(|| {
        format!(
            "installing install docs command launcher '{}'",
            docs_cmd_path.display()
        )
    })?;

    Ok(())
}

fn install_mode_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_experience: InstallExperience,
    interactive_shell: &str,
    ux_docs_frontend: InstallDocsFrontend,
) -> Result<()> {
    let marker_path = rootfs_source_dir.join(CANONICAL_INSTALL_EXPERIENCE_MARKER);
    write_text(&marker_path, &format!("{}\n", install_experience.as_str())).with_context(|| {
        format!(
            "writing install experience marker '{}'",
            marker_path.display()
        )
    })?;
    install_shell_color_profile(rootfs_source_dir).with_context(|| {
        format!(
            "installing shell color profile defaults under '{}'",
            rootfs_source_dir.display()
        )
    })?;

    if install_experience == InstallExperience::Ux {
        let docs_cmd_path = rootfs_source_dir.join("usr/local/bin/levitate-install-docs");
        let docs_tui_cmd_path = rootfs_source_dir.join("usr/local/bin/docs-tui");
        let docs_text_path = rootfs_source_dir.join(CANONICAL_INSTALL_DOCS_TEXT);
        let docs_text = format!(
            "LevitateOS Live Install Tools\n\
             Distro: {distro}\n\
             \n\
             This shell is intended for interactive install preparation.\n\
             Available baseline commands include: recstrap, recfstab, recchroot, sfdisk, mkfs.ext4, ip, ping, curl.\n\
             \n\
             If this host is used for automation, switch to the profile that sets install_experience=automated_ssh.\n",
            distro = distro_id
        );
        write_text(&docs_text_path, &docs_text).with_context(|| {
            format!(
                "writing install docs payload '{}'",
                docs_text_path.display()
            )
        })?;
        match ux_docs_frontend {
            InstallDocsFrontend::BunBundle => {
                install_bun_docs_tui_payload(repo_root, rootfs_source_dir, &docs_cmd_path)
                    .with_context(|| {
                        format!(
                            "installing bun-based install docs payload for '{}'",
                            distro_id
                        )
                    })?
            }
            InstallDocsFrontend::PlainText => {
                let docs_cmd = format!(
                    "#!/bin/sh\n\
set -eu\n\
\n\
DOCS_FILE=\"/usr/local/share/levitate/install-docs.txt\"\n\
case \"${{TERM:-}}\" in\n\
    \"\"|dumb|vt100|vt102|linux)\n\
        export TERM=xterm-256color\n\
        ;;\n\
esac\n\
export COLORTERM=\"${{COLORTERM:-truecolor}}\"\n\
export FORCE_COLOR=\"${{FORCE_COLOR:-3}}\"\n\
if [ -n \"${{NO_COLOR:-}}\" ]; then\n\
    unset NO_COLOR\n\
fi\n\
\n\
if [ -f \"$DOCS_FILE\" ]; then\n\
    if [ \"${{LEVITATE_DOCS_PLAIN:-0}}\" != \"1\" ] && command -v less >/dev/null 2>&1; then\n\
        case \"${{TERM:-}}\" in\n\
            \"\"|dumb|vt100)\n\
                ;;\n\
            *)\n\
                exec less \"$DOCS_FILE\"\n\
                ;;\n\
        esac\n\
    fi\n\
    cat \"$DOCS_FILE\"\n\
else\n\
    echo \"install docs payload missing at $DOCS_FILE\"\n\
fi\n\
\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
                    shell = interactive_shell
                );
                write_executable(&docs_cmd_path, &docs_cmd).with_context(|| {
                    format!(
                        "installing install docs command '{}'",
                        docs_cmd_path.display()
                    )
                })?;
            }
        }

        // Canonical docs-tui command alias for split-pane right-side launchers.
        let docs_tui_cmd = "#!/bin/sh\n\
exec /usr/local/bin/levitate-install-docs \"$@\"\n";
        write_executable(&docs_tui_cmd_path, docs_tui_cmd).with_context(|| {
            format!(
                "installing install docs-tui alias '{}'",
                docs_tui_cmd_path.display()
            )
        })?;
    }

    let entrypoint_path = rootfs_source_dir.join(CANONICAL_INSTALL_ENTRYPOINT);
    let entrypoint_script = match install_experience {
        InstallExperience::Ux => format!(
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
        printf 'install-entrypoint-helper=%s\\n' \"$helper\"\n\
        exit 0\n\
    fi\n\
    printf 'install-entrypoint-helper=none\\n'\n\
    exit 3\n\
fi\n\
\n\
echo \"[{distro}] install experience: UX\"\n\
echo \"Starting local interactive install helper if available...\"\n\
\n\
helper=\"$(choose_install_helper || true)\"\n\
if [ -n \"$helper\" ]; then\n\
    case \"$helper\" in\n\
        */levitate-install-docs-split|levitate-install-docs-split)\n\
            if [ \"${{LEVITATE_INSTALL_ENTRYPOINT_SMOKE:-0}}\" = \"1\" ]; then\n\
                exec \"$helper\" --smoke\n\
            fi\n\
            ;;\n\
    esac\n\
    exec \"$helper\"\n\
fi\n\
\n\
echo \"No local docs TUI binary found; dropping to shell.\"\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
            shell = interactive_shell,
            distro = distro_id
        ),
        InstallExperience::AutomatedSsh => format!(
            "#!/bin/sh\n\
set -eu\n\
\n\
echo \"[{distro}] install experience: automated SSH\"\n\
echo \"This ISO profile is intended for SSH-driven automation (qcow2/.img pipelines).\"\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
            shell = interactive_shell,
            distro = distro_id
        ),
    };
    write_executable(&entrypoint_path, &entrypoint_script).with_context(|| {
        format!(
            "installing install entrypoint script '{}'",
            entrypoint_path.display()
        )
    })?;
    if install_experience == InstallExperience::Ux {
        let ux_profile_path = rootfs_source_dir.join(CANONICAL_INSTALL_UX_PROFILE);
        let ux_profile = "#!/bin/sh\n\
case \"$-\" in\n\
    *i*) ;;\n\
    *) return 0 ;;\n\
esac\n\
\n\
[ -n \"${TMUX:-}\" ] && return 0\n\
[ \"${LEVITATE_INSTALL_UX_LAUNCHED:-0}\" = \"1\" ] && return 0\n\
\n\
if [ -r /run/boot-injection/payload.env ]; then\n\
    while IFS= read -r line; do\n\
        case \"$line\" in\n\
            \"\"|\\#*) continue ;;\n\
            *=*)\n\
                key=\"${line%%=*}\"\n\
                value=\"${line#*=}\"\n\
                case \"$key\" in\n\
                    [A-Za-z_][A-Za-z0-9_]*) export \"$key=$value\" ;;\n\
                esac\n\
                ;;\n\
        esac\n\
    done < /run/boot-injection/payload.env\n\
fi\n\
\n\
TTY=\"$(tty 2>/dev/null || true)\"\n\
if [ -z \"${LEVITATE_INSTALL_LEFT_CMD:-}\" ]; then\n\
    if [ -x /bin/bash ]; then\n\
        LEVITATE_INSTALL_LEFT_CMD=\"/bin/bash -il\"\n\
    else\n\
        LEVITATE_INSTALL_LEFT_CMD=\"/bin/sh -il\"\n\
    fi\n\
    export LEVITATE_INSTALL_LEFT_CMD\n\
fi\n\
if [ \"$TTY\" = \"/dev/tty1\" ]; then\n\
    :\n\
elif [ \"$TTY\" = \"/dev/ttyS0\" ] && [ \"${LEVITATE_INSTALL_SERIAL_UX:-0}\" = \"1\" ]; then\n\
    if [ -z \"${LEVITATE_INSTALL_RIGHT_CMD:-}\" ]; then\n\
        LEVITATE_INSTALL_RIGHT_CMD=\"docs-tui --slug installation\"\n\
        export LEVITATE_INSTALL_RIGHT_CMD\n\
    fi\n\
    export LEVITATE_DOCS_PLAIN=1\n\
    :\n\
else\n\
    return 0\n\
fi\n\
\n\
echo \"[install-ux] Launching install UX on $TTY...\"\n\
export LEVITATE_INSTALL_UX_LAUNCHED=1\n\
exec /usr/local/bin/levitate-install-entrypoint\n";
        write_text(&ux_profile_path, ux_profile).with_context(|| {
            format!(
                "installing install UX profile hook '{}'",
                ux_profile_path.display()
            )
        })?;
    }

    Ok(())
}

fn install_shell_color_profile(rootfs_source_dir: &Path) -> Result<()> {
    let profile_path = rootfs_source_dir.join("etc/profile.d/25-shell-color.sh");
    let profile = "#!/bin/sh\n\
case \"$-\" in\n\
    *i*) ;;\n\
    *) return 0 ;;\n\
esac\n\
\n\
[ \"${NO_COLOR:-0}\" = \"1\" ] && return 0\n\
\n\
if [ -z \"${TERM:-}\" ] || [ \"${TERM:-}\" = \"dumb\" ] || [ \"${TERM:-}\" = \"vt100\" ] || [ \"${TERM:-}\" = \"vt102\" ] || [ \"${TERM:-}\" = \"linux\" ]; then\n\
    TTY=\"$(tty 2>/dev/null || true)\"\n\
    case \"$TTY\" in\n\
        /dev/ttyS*|/dev/tty[0-9]*)\n\
            export TERM=xterm-256color\n\
            ;;\n\
    esac\n\
fi\n\
\n\
export CLICOLOR=\"${CLICOLOR:-1}\"\n\
export COLORTERM=\"${COLORTERM:-truecolor}\"\n\
export LESS=\"${LESS:--FRSX}\"\n\
export PAGER=\"${PAGER:-less}\"\n\
\n\
if command -v dircolors >/dev/null 2>&1; then\n\
    eval \"$(dircolors -b 2>/dev/null || true)\"\n\
fi\n\
\n\
if command -v ls >/dev/null 2>&1; then\n\
    alias ls='ls --color=auto'\n\
    alias ll='ls -alF --color=auto'\n\
    alias la='ls -A --color=auto'\n\
fi\n\
\n\
if command -v grep >/dev/null 2>&1; then\n\
    alias grep='grep --color=auto'\n\
    alias egrep='grep -E --color=auto'\n\
    alias fgrep='grep -F --color=auto'\n\
fi\n\
\n\
if [ -n \"${BASH_VERSION:-}\" ]; then\n\
    export HISTCONTROL=\"${HISTCONTROL:-ignoredups:erasedups}\"\n\
    export HISTSIZE=\"${HISTSIZE:-10000}\"\n\
    export HISTFILESIZE=\"${HISTFILESIZE:-20000}\"\n\
    export HISTTIMEFORMAT=\"${HISTTIMEFORMAT:-%F %T }\"\n\
    shopt -s histappend 2>/dev/null || true\n\
    shopt -s checkwinsize 2>/dev/null || true\n\
    if [ -r /usr/share/bash-completion/bash_completion ]; then\n\
        . /usr/share/bash-completion/bash_completion\n\
    elif [ -r /etc/bash_completion ]; then\n\
        . /etc/bash_completion\n\
    fi\n\
    export PS1=\"\\[\\033[1;32m\\]\\u@\\h\\[\\033[0m\\]:\\[\\033[1;34m\\]\\w\\[\\033[0m\\]\\\\$ \"\n\
fi\n";
    write_text(&profile_path, profile)
        .with_context(|| format!("writing shell color profile '{}'", profile_path.display()))
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
