use anyhow::{bail, Context, Result};
use distro_contract::STAGE_01_REQUIRED_LIVE_SERVICES_BASE;
use serde::Deserialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::stages::s01_boot_inputs::{
    ensure_stage01_required_service_wiring, ensure_systemd_stage01_locale_completeness,
    install_stage_test_scripts, load_s01_boot_input_spec, S01OverlayPolicy,
};
use crate::{
    create_openrc_live_overlay, create_systemd_live_overlay, LiveOverlayConfig,
    SystemdLiveOverlayConfig,
};

const STAGE_MACHINE_ID: &str = "0123456789abcdef0123456789abcdef\n";
const STAGE02_ARTIFACT_TAG: &str = "s02";

#[derive(Debug, Clone)]
pub struct S02LiveToolsInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S02LiveToolsInputSpec {
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    parent_stage: ParentStage,
    overlay: S01OverlayPolicy,
}

#[derive(Debug, Clone, Copy)]
enum ParentStage {
    S01Boot,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02LiveToolsToml {
    stage_02: S02StageToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02StageToml {
    live_tools: S02LiveToolsInputsToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S02LiveToolsInputsToml {
    os_name: String,
}

pub fn load_s02_live_tools_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S02LiveToolsInputSpec> {
    let config_path = variant_dir.join("02LiveTools.toml");
    let config_bytes = fs::read_to_string(&config_path)
        .with_context(|| format!("reading Stage 02 config '{}'", config_path.display()))?;
    let parsed: S02LiveToolsToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing Stage 02 config '{}'", config_path.display()))?;

    let s01_spec =
        load_s01_boot_input_spec(repo_root, variant_dir, distro_id).with_context(|| {
            format!(
                "loading Stage 01 overlay baseline while preparing Stage 02 for '{}'",
                distro_id
            )
        })?;

    Ok(S02LiveToolsInputSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: parsed.stage_02.live_tools.os_name,
        rootfs_source_dir: PathBuf::from("s02-rootfs-source"),
        parent_stage: ParentStage::S01Boot,
        overlay: s01_spec.overlay,
    })
}

pub fn prepare_s02_live_tools_inputs(
    spec: &S02LiveToolsInputSpec,
    output_dir: &Path,
) -> Result<S02LiveToolsInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 02 live tools input output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    let parent_rootfs = resolve_parent_rootfs_image(spec.parent_stage, output_dir)?;
    extract_erofs_rootfs(&parent_rootfs, &rootfs_source_dir).with_context(|| {
        format!(
            "extracting parent stage rootfs from '{}'",
            parent_rootfs.display()
        )
    })?;

    add_stage02_required_tools(&spec.repo_root, &rootfs_source_dir, &spec.distro_id)
        .with_context(|| format!("adding Stage 02 required tools for '{}'", spec.distro_id))?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into Stage 02 rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_stage01_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 02 locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let stage_issue_banner = stage_issue_banner(&spec.os_name, "S02 Live Tools");
    let live_overlay_dir = match &spec.overlay {
        S01OverlayPolicy::Systemd { issue_message } => create_systemd_live_overlay(
            output_dir,
            &SystemdLiveOverlayConfig {
                os_name: &spec.os_name,
                issue_message: issue_message
                    .as_deref()
                    .or(Some(stage_issue_banner.as_str())),
                masked_units: &[],
                write_serial_test_profile: true,
                machine_id: Some(STAGE_MACHINE_ID),
                enforce_utf8_locale_profile: true,
            },
        )
        .with_context(|| format!("creating systemd live overlay for {}", spec.distro_id))?,
        S01OverlayPolicy::OpenRc {
            inittab,
            profile_overlay,
        } => create_openrc_live_overlay(
            output_dir,
            &LiveOverlayConfig {
                os_name: &spec.os_name,
                inittab: *inittab,
                profile_overlay: profile_overlay.as_deref(),
                issue_message: Some(stage_issue_banner.as_str()),
            },
        )
        .with_context(|| format!("creating openrc live overlay for {}", spec.distro_id))?,
    };
    let live_overlay_dir =
        rename_live_overlay_for_stage(output_dir, &live_overlay_dir, STAGE02_ARTIFACT_TAG)
            .with_context(|| {
                format!(
                    "renaming Stage 02 live overlay directory for '{}'",
                    spec.distro_id
                )
            })?;

    let required_services = STAGE_01_REQUIRED_LIVE_SERVICES_BASE
        .iter()
        .map(|svc| (*svc).to_string())
        .collect::<Vec<_>>();
    ensure_stage01_required_service_wiring(&live_overlay_dir, &spec.overlay, &required_services)
        .with_context(|| {
            format!(
                "ensuring Stage 01 service wiring in 02LiveTools overlay for '{}'",
                spec.distro_id
            )
        })?;

    Ok(S02LiveToolsInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

fn stage_issue_banner(os_name: &str, stage_label: &str) -> String {
    format!(
        "\n{} {} Live - \\l\n\nLogin as 'root' (no password)\n\n",
        os_name, stage_label
    )
}

fn add_stage02_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
) -> Result<()> {
    let target = match distro_id {
        "levitate" | "ralph" => None,
        "acorn" | "iuppiter" => Some("x86_64-unknown-linux-musl"),
        other => bail!(
            "unsupported distro id for Stage 02 tool wiring: '{}'",
            other
        ),
    };

    if matches!(distro_id, "acorn" | "iuppiter") {
        ensure_musl_stage02_packages(rootfs_source_dir, distro_id).with_context(|| {
            format!(
                "installing Stage 02 musl package additions for '{}'",
                distro_id
            )
        })?;
    }

    let dest_dirs = [
        rootfs_source_dir.join("usr/bin"),
        rootfs_source_dir.join("bin"),
    ];
    for dest_dir in &dest_dirs {
        fs::create_dir_all(dest_dir).with_context(|| {
            format!(
                "creating Stage 02 tool destination '{}'",
                dest_dir.display()
            )
        })?;
    }

    for tool in ["recstrap", "recfstab", "recchroot"] {
        let built = build_workspace_tool(repo_root, tool, target)
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
                    "copying Stage 02 tool '{}' -> '{}'",
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

fn ensure_musl_stage02_packages(rootfs_source_dir: &Path, distro_id: &str) -> Result<()> {
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
                "running apk Stage 02 package install in rootfs '{}'",
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

fn build_workspace_tool(
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

fn create_unique_output_dir(output_dir: &Path, logical_name: &Path) -> Result<PathBuf> {
    let stem = logical_name
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("sxx-rootfs-source");
    let path = output_dir.join(stem);
    if path.exists() {
        fs::remove_dir_all(&path).with_context(|| {
            format!(
                "removing existing stage rootfs directory before recreation '{}'",
                path.display()
            )
        })?;
    }
    fs::create_dir_all(&path)
        .with_context(|| format!("creating stage rootfs directory '{}'", path.display()))?;
    Ok(path)
}

fn rename_live_overlay_for_stage(
    output_dir: &Path,
    source_overlay: &Path,
    stage_artifact_tag: &str,
) -> Result<PathBuf> {
    let target_overlay = output_dir.join(format!("{stage_artifact_tag}-live-overlay"));
    if source_overlay == target_overlay {
        return Ok(target_overlay);
    }
    if target_overlay.exists() {
        fs::remove_dir_all(&target_overlay).with_context(|| {
            format!(
                "removing pre-existing stage live overlay '{}'",
                target_overlay.display()
            )
        })?;
    }
    fs::rename(source_overlay, &target_overlay).with_context(|| {
        format!(
            "renaming live overlay '{}' -> '{}'",
            source_overlay.display(),
            target_overlay.display()
        )
    })?;
    Ok(target_overlay)
}

fn resolve_parent_rootfs_image(parent_stage: ParentStage, output_dir: &Path) -> Result<PathBuf> {
    let distro_output = output_dir.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve distro output directory from stage output '{}'",
            output_dir.display()
        )
    })?;
    let path = match parent_stage {
        ParentStage::S01Boot => distro_output.join("s01-boot/s01-filesystem.erofs"),
    };
    if !path.is_file() {
        bail!(
            "missing parent stage rootfs image '{}'; build parent stage first",
            path.display()
        );
    }
    Ok(path)
}

fn extract_erofs_rootfs(image: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        fs::remove_dir_all(destination).with_context(|| {
            format!(
                "removing incomplete rootfs source directory '{}'",
                destination.display()
            )
        })?;
    }
    fs::create_dir_all(destination).with_context(|| {
        format!(
            "creating rootfs source destination directory '{}'",
            destination.display()
        )
    })?;

    let extract_arg = format!("--extract={}", destination.display());
    let output = Command::new("fsck.erofs")
        .arg(extract_arg)
        .arg(image)
        .output()
        .with_context(|| format!("running fsck.erofs for '{}'", image.display()))?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "fsck.erofs failed extracting '{}' into '{}': {}\n{}",
        image.display(),
        destination.display(),
        stdout.trim(),
        stderr.trim()
    )
}
