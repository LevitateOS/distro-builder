use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;

const LEGACY_ROOTFS_COMPONENT_SEQUENCES: &[&[&str]] = &[
    &["leviso", "downloads", "rootfs"],
    &["ralphos", "downloads", "rootfs"],
    &["acornos", "downloads", "rootfs"],
    &["iuppiteros", "downloads", "rootfs"],
];

#[derive(Debug, Clone)]
pub(crate) struct ProducerPlan {
    pub(crate) source_rootfs_dir: Option<PathBuf>,
    pub(crate) producers: Vec<RootfsProducer>,
}

#[derive(Debug, Clone)]
pub(crate) enum RootfsProducer {
    CopyTree {
        source: PathBuf,
        destination: PathBuf,
    },
    CopySymlink {
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

pub(crate) fn build_baseline_producers(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
) -> Vec<RootfsProducer> {
    let os_release = format!(
        "NAME=\"{}\"\nID={}\nPRETTY_NAME=\"{} (Stage 00Build)\"\n",
        os_name, os_id, os_name
    );
    let stage_manifest = format!(
        "{{\n  \"schema\": 1,\n  \"stage\": \"00Build\",\n  \"stage_slug\": \"s00_build\",\n  \"distro_id\": \"{}\",\n  \"os_name\": \"{}\",\n  \"os_id\": \"{}\",\n  \"payload_role\": \"rootfs-source\"\n}}\n",
        distro_id, os_name, os_id
    );
    vec![
        RootfsProducer::WriteText {
            path: PathBuf::from("usr/lib/stage-manifest.json"),
            content: stage_manifest,
            mode: None,
        },
        RootfsProducer::WriteText {
            path: PathBuf::from("etc/os-release"),
            content: os_release,
            mode: None,
        },
    ]
}

pub(crate) fn boot_baseline_producers(overlay_kind: &str) -> Vec<RootfsProducer> {
    if overlay_kind == "systemd" {
        return vec![
            RootfsProducer::WriteText {
                path: PathBuf::from(".live-payload-role"),
                content: "rootfs\n".to_string(),
                mode: None,
            },
            RootfsProducer::CopySymlink {
                source: PathBuf::from("bin"),
                destination: PathBuf::from("bin"),
            },
            RootfsProducer::CopySymlink {
                source: PathBuf::from("sbin"),
                destination: PathBuf::from("sbin"),
            },
            RootfsProducer::CopySymlink {
                source: PathBuf::from("lib"),
                destination: PathBuf::from("lib"),
            },
            RootfsProducer::CopySymlink {
                source: PathBuf::from("lib64"),
                destination: PathBuf::from("lib64"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/systemd"),
                destination: PathBuf::from("usr/lib/systemd"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/tmpfiles.d"),
                destination: PathBuf::from("usr/lib/tmpfiles.d"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/udev"),
                destination: PathBuf::from("usr/lib/udev"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/lib/kbd"),
                destination: PathBuf::from("usr/lib/kbd"),
            },
            RootfsProducer::CopyFile {
                source: PathBuf::from("usr/lib/locale/C.utf8/LC_CTYPE"),
                destination: PathBuf::from("usr/lib/locale/C.utf8/LC_CTYPE"),
                optional: false,
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
                source: PathBuf::from("usr/libexec"),
                destination: PathBuf::from("usr/libexec"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("usr/share/dbus-1"),
                destination: PathBuf::from("usr/share/dbus-1"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("etc"),
                destination: PathBuf::from("etc"),
            },
            RootfsProducer::CopyTree {
                source: PathBuf::from("var"),
                destination: PathBuf::from("var"),
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
        ];
    }
    vec![
        RootfsProducer::WriteText {
            path: PathBuf::from(".live-payload-role"),
            content: "rootfs\n".to_string(),
            mode: None,
        },
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

pub(crate) fn apply_producer_plan(plan: &ProducerPlan, destination_root: &Path) -> Result<()> {
    if let Some(source_root) = plan.source_rootfs_dir.as_ref() {
        ensure_non_legacy_rootfs_source(source_root).with_context(|| {
            format!(
                "applying producer plan with source rootfs '{}'",
                source_root.display()
            )
        })?;
    } else if plan.producers.iter().any(|producer| {
        matches!(
            producer,
            RootfsProducer::CopyTree { .. }
                | RootfsProducer::CopySymlink { .. }
                | RootfsProducer::CopyFile { .. }
        )
    }) {
        bail!(
            "Stage 01 producer plan requires copy-based rootfs source, but no non-legacy source_rootfs_dir is configured.\n\
             Legacy */downloads/rootfs mappings are intentionally forbidden.\n\
             Migrate Stage 01 payload assembly to non-legacy staged producers."
        );
    }

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
            RootfsProducer::CopySymlink {
                source,
                destination,
            } => {
                let source_root = plan.source_rootfs_dir.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("copy_symlink producer requires source_rootfs_dir to be set")
                })?;
                let source_path = source_root.join(source);
                let source_meta = fs::symlink_metadata(&source_path).with_context(|| {
                    format!("reading symlink metadata for '{}'", source_path.display())
                })?;
                if !source_meta.file_type().is_symlink() {
                    bail!(
                        "copy_symlink source '{}' is not a symlink",
                        source_path.display()
                    );
                }
                let link_target = fs::read_link(&source_path).with_context(|| {
                    format!("reading link target for '{}'", source_path.display())
                })?;
                let target_path = destination_root.join(destination);
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "creating destination parent for copy_symlink '{}'",
                            parent.display()
                        )
                    })?;
                }
                if target_path.exists() {
                    let meta = fs::symlink_metadata(&target_path).with_context(|| {
                        format!(
                            "reading destination metadata for '{}'",
                            target_path.display()
                        )
                    })?;
                    if meta.file_type().is_dir() && !meta.file_type().is_symlink() {
                        fs::remove_dir_all(&target_path).with_context(|| {
                            format!(
                                "removing existing directory destination '{}'",
                                target_path.display()
                            )
                        })?;
                    } else {
                        fs::remove_file(&target_path).with_context(|| {
                            format!(
                                "removing existing file/symlink destination '{}'",
                                target_path.display()
                            )
                        })?;
                    }
                }
                symlink(&link_target, &target_path).with_context(|| {
                    format!(
                        "creating symlink '{}' -> '{}'",
                        target_path.display(),
                        link_target.display()
                    )
                })?;
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

pub(crate) fn ensure_non_legacy_rootfs_source(path: &Path) -> Result<()> {
    if !is_legacy_rootfs_source(path) {
        return Ok(());
    }

    bail!(
        "policy violation: legacy rootfs source '{}' is forbidden.\n\
         Legacy distro crate rootfs trees must not be consumed by distro-builder stage inputs.\n\
         Provide a non-legacy stage source path (for example under '.artifacts/out/<distro>/sNN-*/' or 'distro-variants/<distro>/').",
        path.display()
    );
}

fn is_legacy_rootfs_source(path: &Path) -> bool {
    let components: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();

    LEGACY_ROOTFS_COMPONENT_SEQUENCES
        .iter()
        .any(|needle| contains_component_sequence(&components, needle))
}

fn contains_component_sequence(haystack: &[String], needle: &[&str]) -> bool {
    if needle.is_empty() || needle.len() > haystack.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| window.iter().map(String::as_str).eq(needle.iter().copied()))
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
