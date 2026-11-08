use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Component;
use std::path::{Path, PathBuf};

const LEGACY_ROOTFS_COMPONENT_SEQUENCES: &[&[&str]] = &[
    &["leviso", "downloads", "rootfs"],
    &["acornos", "downloads", "rootfs"],
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
    WriteText {
        path: PathBuf,
        content: String,
    },
}

pub(crate) fn build_baseline_producers(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    profile: &distro_contract::Profile,
) -> Vec<RootfsProducer> {
    // The image's identity and its distro profile (the REC_* fields the install
    // tools read on the target).
    let os_release = format!(
        "NAME=\"{}\"\nID={}\nPRETTY_NAME=\"{}\"\n{}",
        os_name,
        os_id,
        os_name,
        profile.os_release_lines()
    );
    let product_manifest = format!(
        "{{\n  \"schema\": 1,\n  \"product\": \"base-rootfs\",\n  \"product_slug\": \"base_rootfs\",\n  \"distro_id\": \"{}\",\n  \"os_name\": \"{}\",\n  \"os_id\": \"{}\",\n  \"payload_role\": \"rootfs-source\"\n}}\n",
        distro_id, os_name, os_id
    );
    vec![
        RootfsProducer::WriteText {
            path: PathBuf::from("usr/lib/product-manifest.json"),
            content: product_manifest,
        },
        RootfsProducer::WriteText {
            path: PathBuf::from("etc/os-release"),
            content: os_release,
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
    } else if plan
        .producers
        .iter()
        .any(|producer| matches!(producer, RootfsProducer::CopyTree { .. }))
    {
        bail!(
            "live-boot product preparation requires copy-based rootfs sourcing, but no non-legacy source_rootfs_dir is configured.\n\
             Legacy */downloads/rootfs mappings are intentionally forbidden.\n\
             Migrate payload assembly to canonical non-legacy producers."
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
                levitate_recipe::target::merge_tree(&source_path, &target_path)?;
            }
            RootfsProducer::WriteText { path, content } => {
                let target = destination_root.join(path);
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!(
                            "creating destination parent for write_text '{}'",
                            parent.display()
                        )
                    })?;
                }
                fs::write(&target, content).with_context(|| {
                    format!("writing product rootfs file '{}'", target.display())
                })?;
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
         Legacy distro crate rootfs trees must not be consumed by distro-builder product inputs.\n\
         Provide a non-legacy product source path (for example under '.artifacts/prepared/<distro>/<product>/' or 'distro/variants/<distro>/').",
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
