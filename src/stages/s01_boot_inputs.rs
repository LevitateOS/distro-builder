use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
#[cfg(test)]
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    create_openrc_live_overlay, create_systemd_live_overlay, InittabVariant, LiveOverlayConfig,
    SystemdLiveOverlayConfig,
};

#[derive(Debug, Clone)]
pub struct S00BuildInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S01BootInputs {
    pub rootfs_source_dir: PathBuf,
    pub live_overlay_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub enum S01OverlayPolicy {
    Systemd {
        issue_message: Option<String>,
    },
    OpenRc {
        inittab: InittabVariant,
        profile_overlay: Option<PathBuf>,
    },
}

#[derive(Debug, Clone)]
pub struct S00BuildInputSpec {
    pub distro_id: String,
    pub os_name: String,
    pub os_id: String,
    pub rootfs_source_dir: PathBuf,
    plan: ProducerPlan,
}

#[derive(Debug, Clone)]
pub struct S01BootInputSpec {
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    parent_stage: ParentStage,
    add_plan: ProducerPlan,
    remove_exceptions: Vec<RemovalException>,
    pub overlay: S01OverlayPolicy,
}

#[derive(Debug, Clone, Copy)]
enum ParentStage {
    S00Build,
}

#[derive(Debug, Clone)]
struct ProducerPlan {
    source_rootfs_dir: Option<PathBuf>,
    producers: Vec<RootfsProducer>,
}

#[derive(Debug, Clone)]
enum RootfsProducer {
    CopyTree {
        source: PathBuf,
        destination: PathBuf,
    },
    CopyFile {
        source: PathBuf,
        destination: PathBuf,
        optional: bool,
    },
    WriteText {
        path: PathBuf,
        content: String,
        mode: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct RemovalException {
    path: PathBuf,
    reason: String,
    ticket_or_ref: String,
    expires_at_stage: Option<String>,
    expires_at_date: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S01BootToml {
    stage_01: S01StageToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S01StageToml {
    boot_inputs: S01BootInputsToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct S01BootInputsToml {
    os_name: String,
    overlay_kind: String,
    openrc_inittab: Option<String>,
    profile_overlay: Option<String>,
    issue_message: Option<String>,
}

pub fn load_s00_build_input_spec(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    _output_root: &Path,
) -> Result<S00BuildInputSpec> {
    Ok(S00BuildInputSpec {
        distro_id: distro_id.to_string(),
        os_name: os_name.to_string(),
        os_id: os_id.to_string(),
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: stage00_baseline_producers(os_name, os_id),
        },
    })
}

pub fn prepare_s00_build_inputs(
    spec: &S00BuildInputSpec,
    output_dir: &Path,
) -> Result<S00BuildInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 00 build input output directory '{}'",
            output_dir.display()
        )
    })?;

    let rootfs_source_dir = create_unique_output_dir(output_dir, &spec.rootfs_source_dir)?;
    apply_producer_plan(&spec.plan, &rootfs_source_dir)
        .with_context(|| format!("materializing Stage 00 rootfs for '{}'", spec.distro_id))?;
    write_stage_marker(&rootfs_source_dir, "00Build")?;

    let live_overlay_dir = create_empty_overlay(output_dir)
        .with_context(|| format!("creating empty overlay for {}", spec.distro_id))?;

    Ok(S00BuildInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

pub fn load_s01_boot_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01BootInputSpec> {
    let config_path = variant_dir.join("01Boot.toml");
    let config_bytes = fs::read_to_string(&config_path)
        .with_context(|| format!("reading Stage 01 config '{}'", config_path.display()))?;
    let parsed: S01BootToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing Stage 01 config '{}'", config_path.display()))?;

    let boot_inputs = parsed.stage_01.boot_inputs;

    let overlay_kind = boot_inputs.overlay_kind.trim().to_ascii_lowercase();
    let overlay = match overlay_kind.as_str() {
        "systemd" => S01OverlayPolicy::Systemd {
            issue_message: boot_inputs.issue_message,
        },
        "openrc" => {
            let inittab = parse_openrc_inittab(
                boot_inputs.openrc_inittab.as_deref(),
                &config_path,
                distro_id,
            )?;
            let profile_overlay = boot_inputs
                .profile_overlay
                .as_ref()
                .map(|path| resolve_repo_path(repo_root, path));

            S01OverlayPolicy::OpenRc {
                inittab,
                profile_overlay,
            }
        }
        other => bail!(
            "invalid Stage 01 config '{}': unsupported overlay_kind '{}' (expected 'systemd' or 'openrc')",
            config_path.display(),
            other
        ),
    };

    let parent_stage = ParentStage::S00Build;
    let source_rootfs_dir = stage01_source_rootfs_dir(repo_root, distro_id)?;
    let add_producers = stage01_baseline_producers(&overlay_kind);

    Ok(S01BootInputSpec {
        distro_id: distro_id.to_string(),
        os_name: boot_inputs.os_name,
        rootfs_source_dir: PathBuf::from("rootfs-source"),
        parent_stage,
        add_plan: ProducerPlan {
            source_rootfs_dir: Some(source_rootfs_dir),
            producers: add_producers,
        },
        remove_exceptions: Vec::new(),
        overlay,
    })
}

pub fn prepare_s01_boot_inputs(
    spec: &S01BootInputSpec,
    output_dir: &Path,
) -> Result<S01BootInputs> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "creating Stage 01 boot input output directory '{}'",
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

    apply_producer_plan(&spec.add_plan, &rootfs_source_dir).with_context(|| {
        format!(
            "applying Stage 01 additive producers for '{}'",
            spec.distro_id
        )
    })?;
    apply_remove_exceptions(&rootfs_source_dir, &spec.remove_exceptions).with_context(|| {
        format!(
            "applying Stage 01 remove exceptions for '{}'",
            spec.distro_id
        )
    })?;

    write_stage_marker(&rootfs_source_dir, "01Boot")?;

    let live_overlay_dir = match &spec.overlay {
        S01OverlayPolicy::Systemd { issue_message } => create_systemd_live_overlay(
            output_dir,
            &SystemdLiveOverlayConfig {
                os_name: &spec.os_name,
                issue_message: issue_message.as_deref(),
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
            },
        )
        .with_context(|| format!("creating openrc live overlay for {}", spec.distro_id))?,
    };

    Ok(S01BootInputs {
        rootfs_source_dir,
        live_overlay_dir,
    })
}

fn parse_openrc_inittab(
    value: Option<&str>,
    config_path: &Path,
    distro_id: &str,
) -> Result<InittabVariant> {
    let raw = value.ok_or_else(|| {
        anyhow::anyhow!(
            "invalid Stage 01 config '{}': openrc_inittab is required for distro '{}'",
            config_path.display(),
            distro_id
        )
    })?;

    match raw.trim().to_ascii_lowercase().as_str() {
        "desktop_with_serial" => Ok(InittabVariant::DesktopWithSerial),
        "serial_only" => Ok(InittabVariant::SerialOnly),
        other => bail!(
            "invalid Stage 01 config '{}': unsupported openrc_inittab '{}' for distro '{}'",
            config_path.display(),
            other,
            distro_id
        ),
    }
}

fn stage01_source_rootfs_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let relative = match distro_id {
        "levitate" => "leviso/downloads/rootfs",
        "ralph" => "RalphOS/downloads/rootfs",
        "acorn" => "AcornOS/downloads/rootfs",
        "iuppiter" => "IuppiterOS/downloads/rootfs",
        other => bail!(
            "unsupported distro '{}' for Stage 01 source rootfs mapping",
            other
        ),
    };
    Ok(resolve_repo_path(repo_root, relative))
}

fn stage00_baseline_producers(os_name: &str, os_id: &str) -> Vec<RootfsProducer> {
    let os_release = format!(
        "NAME=\"{}\"\nID={}\nPRETTY_NAME=\"{} (Stage 00Build)\"\n",
        os_name, os_id, os_name
    );
    vec![
        RootfsProducer::WriteText {
            path: PathBuf::from(".buildstamp"),
            content: "00Build\n".to_string(),
            mode: None,
        },
        RootfsProducer::WriteText {
            path: PathBuf::from("etc/os-release"),
            content: os_release.clone(),
            mode: None,
        },
        RootfsProducer::WriteText {
            path: PathBuf::from("usr/lib/os-release"),
            content: os_release,
            mode: None,
        },
    ]
}

fn stage01_baseline_producers(overlay_kind: &str) -> Vec<RootfsProducer> {
    if overlay_kind == "systemd" {
        return vec![
            RootfsProducer::CopyTree {
                source: PathBuf::from("bin"),
                destination: PathBuf::from("bin"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("sbin"),
                destination: PathBuf::from("sbin"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("lib"),
                destination: PathBuf::from("lib"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("lib64"),
                destination: PathBuf::from("lib64"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/systemd"),
                destination: PathBuf::from("usr/lib/systemd"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/udev"),
                destination: PathBuf::from("usr/lib/udev"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib64"),
                destination: PathBuf::from("usr/lib64"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/bin"),
                destination: PathBuf::from("usr/bin"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/sbin"),
                destination: PathBuf::from("usr/sbin"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("etc/systemd"),
                destination: PathBuf::from("etc/systemd"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("etc/udev"),
                destination: PathBuf::from("etc/udev"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("etc/pam.d"),
                destination: PathBuf::from("etc/pam.d"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("var/empty"),
                destination: PathBuf::from("var/empty"),
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/lib/systemd/systemd"),
                destination: PathBuf::from("usr/lib/systemd/systemd"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/sbin/agetty"),
                destination: PathBuf::from("usr/sbin/agetty"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/login"),
                destination: PathBuf::from("usr/bin/login"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/bash"),
                destination: PathBuf::from("usr/bin/bash"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/sh"),
                destination: PathBuf::from("usr/bin/sh"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/mount"),
                destination: PathBuf::from("usr/bin/mount"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/umount"),
                destination: PathBuf::from("usr/bin/umount"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/systemd-tmpfiles"),
                destination: PathBuf::from("usr/bin/systemd-tmpfiles"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/bin/udevadm"),
                destination: PathBuf::from("usr/bin/udevadm"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/sbin/modprobe"),
                destination: PathBuf::from("usr/sbin/modprobe"),
                optional: false,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/nsswitch.conf"),
                destination: PathBuf::from("etc/nsswitch.conf"),
                optional: true,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/passwd"),
                destination: PathBuf::from("etc/passwd"),
                optional: true,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/group"),
                destination: PathBuf::from("etc/group"),
                optional: true,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/login.defs"),
                destination: PathBuf::from("etc/login.defs"),
                optional: true,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/shells"),
                destination: PathBuf::from("etc/shells"),
                optional: true,
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("etc/hosts"),
                destination: PathBuf::from("etc/hosts"),
                optional: true,
            },
        ];
    }
    vec![
        RootfsProducer::CopyTree {
            source: PathBuf::from("bin"),
            destination: PathBuf::from("bin"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("sbin"),
            destination: PathBuf::from("sbin"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("lib"),
            destination: PathBuf::from("lib"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("etc"),
            destination: PathBuf::from("etc"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("usr/bin"),
            destination: PathBuf::from("usr/bin"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("usr/sbin"),
            destination: PathBuf::from("usr/sbin"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("usr/lib"),
            destination: PathBuf::from("usr/lib"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("usr/libexec"),
            destination: PathBuf::from("usr/libexec"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("var/empty"),
            destination: PathBuf::from("var/empty"),
        },
        RootfsProducer::CopyTree {
            source: PathBuf::from("var/lib"),
            destination: PathBuf::from("var/lib"),
        },
    ]
}

fn create_unique_output_dir(output_dir: &Path, logical_name: &Path) -> Result<PathBuf> {
    let stem = logical_name
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("rootfs-source");
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let unique = format!("{}-{}-{}", stem, std::process::id(), ts);
    let path = output_dir.join(unique);
    fs::create_dir_all(&path).with_context(|| {
        format!(
            "creating unique stage rootfs directory '{}'",
            path.display()
        )
    })?;
    Ok(path)
}

fn resolve_repo_path(repo_root: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        repo_root.join(candidate)
    }
}

fn resolve_parent_rootfs_image(parent_stage: ParentStage, output_dir: &Path) -> Result<PathBuf> {
    let distro_output = output_dir.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "cannot resolve distro output directory from stage output '{}'",
            output_dir.display()
        )
    })?;
    let path = match parent_stage {
        ParentStage::S00Build => distro_output.join("s00-build/s00-filesystem.erofs"),
    };
    if !path.is_file() {
        bail!(
            "missing parent stage rootfs image '{}'; build parent stage first",
            path.display()
        );
    }
    Ok(path)
}

fn apply_producer_plan(plan: &ProducerPlan, destination_root: &Path) -> Result<()> {
    for producer in &plan.producers {
        match producer {
            RootfsProducer::CopyTree {
                source,
                destination,
            } => {
                let source_root = plan.source_rootfs_dir.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("copy_tree producer requires source_rootfs_dir to be set")
                })?;
                let source_path = source_root.join(source);
                if !source_path.is_dir() {
                    bail!(
                        "copy_tree source '{}' is not a directory",
                        source_path.display()
                    );
                }
                let target_path = destination_root.join(destination);
                fs::create_dir_all(&target_path).with_context(|| {
                    format!(
                        "creating destination directory for copy_tree '{}'",
                        target_path.display()
                    )
                })?;
                rsync_tree(&source_path, &target_path)?;
            }
            RootfsProducer::CopyFile {
                source,
                destination,
                optional,
            } => {
                let source_root = plan.source_rootfs_dir.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("copy_file producer requires source_rootfs_dir to be set")
                })?;
                let source_path = source_root.join(source);
                if !source_path.is_file() {
                    if *optional {
                        continue;
                    }
                    bail!("copy_file source '{}' not found", source_path.display());
                }
                let target_path = destination_root.join(destination);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "creating destination parent for copy_file '{}'",
                            parent.display()
                        )
                    })?;
                }
                fs::copy(&source_path, &target_path).with_context(|| {
                    format!(
                        "copying file from '{}' to '{}'",
                        source_path.display(),
                        target_path.display()
                    )
                })?;
            }
            RootfsProducer::WriteText {
                path,
                content,
                mode,
            } => {
                let target = destination_root.join(path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "creating destination parent for write_text '{}'",
                            parent.display()
                        )
                    })?;
                }
                fs::write(&target, content)
                    .with_context(|| format!("writing stage rootfs file '{}'", target.display()))?;
                if let Some(mode) = mode {
                    let mut perms = fs::metadata(&target)
                        .with_context(|| format!("reading file metadata '{}'", target.display()))?
                        .permissions();
                    perms.set_mode(*mode);
                    fs::set_permissions(&target, perms).with_context(|| {
                        format!("setting file permissions on '{}'", target.display())
                    })?;
                }
            }
        }
    }

    Ok(())
}

fn rsync_tree(source_dir: &Path, destination_dir: &Path) -> Result<()> {
    let output = Command::new("rsync")
        .arg("-a")
        .arg(format!("{}/", source_dir.display()))
        .arg(format!("{}/", destination_dir.display()))
        .output()
        .with_context(|| {
            format!(
                "running rsync from '{}' to '{}'",
                source_dir.display(),
                destination_dir.display()
            )
        })?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "rsync failed from '{}' to '{}': {}\n{}",
        source_dir.display(),
        destination_dir.display(),
        stdout.trim(),
        stderr.trim()
    )
}

fn apply_remove_exceptions(
    rootfs_source_dir: &Path,
    remove_exceptions: &[RemovalException],
) -> Result<()> {
    for rule in remove_exceptions {
        let _audit = (
            &rule.reason,
            &rule.ticket_or_ref,
            &rule.expires_at_stage,
            &rule.expires_at_date,
        );
        let target = rootfs_source_dir.join(&rule.path);
        if !target.exists() {
            continue;
        }
        if target.is_dir() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("removing exception directory '{}'", target.display()))?;
        } else {
            fs::remove_file(&target)
                .with_context(|| format!("removing exception file '{}'", target.display()))?;
        }
    }
    Ok(())
}

fn write_stage_marker(rootfs_source_dir: &Path, stage_name: &str) -> Result<()> {
    let marker_path = rootfs_source_dir.join("etc/levitate-stage");
    let marker_parent = marker_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "invalid marker path without parent: '{}'",
            marker_path.display()
        )
    })?;
    fs::create_dir_all(marker_parent).with_context(|| {
        format!(
            "creating rootfs marker parent directory '{}'",
            marker_parent.display()
        )
    })?;
    fs::write(&marker_path, format!("{stage_name}\n"))
        .with_context(|| format!("writing stage marker file '{}'", marker_path.display()))?;
    Ok(())
}

fn create_empty_overlay(output_dir: &Path) -> Result<PathBuf> {
    let live_overlay = output_dir.join("live-overlay");
    if live_overlay.exists() {
        fs::remove_dir_all(&live_overlay).with_context(|| {
            format!(
                "removing existing live overlay directory '{}'",
                live_overlay.display()
            )
        })?;
    }
    fs::create_dir_all(&live_overlay).with_context(|| {
        format!(
            "creating empty live overlay directory '{}'",
            live_overlay.display()
        )
    })?;
    Ok(live_overlay)
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
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "fsck.erofs failed for '{}': {}\n{}",
            image.display(),
            stdout.trim(),
            stderr.trim()
        );
    }
    Ok(())
}

#[cfg(test)]
fn parse_relative_path(raw: &str, field: &str) -> Result<PathBuf> {
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        bail!("{field} must be relative, got absolute path '{}'", raw);
    }
    for component in candidate.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            bail!(
                "{field} contains invalid traversal/root component in '{}'",
                raw
            );
        }
    }
    Ok(candidate.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_relative_path_rejects_parent_traversal() {
        let result = parse_relative_path("../etc/passwd", "test");
        assert!(result.is_err());
    }

    #[test]
    fn stage00_baseline_contains_os_release_files() {
        let producers = stage00_baseline_producers("LevitateOS", "levitateos");
        let paths: Vec<PathBuf> = producers
            .iter()
            .filter_map(|p| match p {
                RootfsProducer::WriteText { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(paths.contains(&PathBuf::from("etc/os-release")));
        assert!(paths.contains(&PathBuf::from("usr/lib/os-release")));
    }
}
