use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recipe::rocky_stage01::{
    materialize_rootfs, materialize_rootfs_from_recipe, RockyStage01RecipeSpec,
    Stage01RootfsRecipeSpec,
};
use crate::{
    create_openrc_live_overlay, create_systemd_live_overlay, InittabVariant, LiveOverlayConfig,
    SystemdLiveOverlayConfig,
};

const STAGE_MACHINE_ID: &str = "0123456789abcdef0123456789abcdef\n";
const STAGE00_ARTIFACT_TAG: &str = "s00";
const STAGE01_ARTIFACT_TAG: &str = "s01";
const LEGACY_ROOTFS_COMPONENT_SEQUENCES: &[&[&str]] = &[
    &["leviso", "downloads", "rootfs"],
    &["ralphos", "downloads", "rootfs"],
    &["acornos", "downloads", "rootfs"],
    &["iuppiteros", "downloads", "rootfs"],
];

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
    repo_root: PathBuf,
    pub distro_id: String,
    pub os_name: String,
    pub rootfs_source_dir: PathBuf,
    parent_stage: ParentStage,
    add_plan: ProducerPlan,
    required_services: Vec<String>,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub overlay: S01OverlayPolicy,
}

#[derive(Debug, Clone)]
enum S01RootfsSourcePolicy {
    RecipeRocky {
        recipe_script: PathBuf,
        iso_name: String,
        sha256: String,
        sha256_url: String,
        torrent_url: String,
    },
    RecipeCustom {
        recipe_script: PathBuf,
        defines: BTreeMap<String, String>,
    },
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
    required_services: Option<Vec<String>>,
    rootfs_source: Option<S01RootfsSourceToml>,
    openrc_inittab: Option<String>,
    profile_overlay: Option<String>,
    issue_message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct S01RootfsSourceToml {
    kind: String,
    recipe_script: String,
    iso_name: Option<String>,
    sha256: Option<String>,
    sha256_url: Option<String>,
    torrent_url: Option<String>,
    defines: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
struct StageRunMetadataFile {
    run_id: String,
    status: String,
    created_at_utc: String,
    finished_at_utc: Option<String>,
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
        rootfs_source_dir: PathBuf::from("s00-rootfs-source"),
        plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: stage00_baseline_producers(distro_id, os_name, os_id),
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
    let mut required_services = boot_inputs
        .required_services
        .unwrap_or_else(|| vec!["sshd".to_string()])
        .into_iter()
        .map(|service| service.trim().to_ascii_lowercase())
        .filter(|service| !service.is_empty())
        .collect::<Vec<_>>();
    required_services.sort();
    required_services.dedup();
    if !required_services.iter().any(|svc| svc == "sshd") {
        bail!(
            "invalid Stage 01 config '{}': required_services must include 'sshd' (OpenSSH is first-class in Stage 01)",
            config_path.display()
        );
    }

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
    let rootfs_source_policy = parse_stage01_rootfs_source_policy(
        repo_root,
        &config_path,
        boot_inputs.rootfs_source.clone(),
    )?;
    let mut add_producers = stage01_baseline_producers(&overlay_kind);
    if rootfs_source_policy.is_none() {
        add_producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
    }

    Ok(S01BootInputSpec {
        repo_root: repo_root.to_path_buf(),
        distro_id: distro_id.to_string(),
        os_name: boot_inputs.os_name,
        rootfs_source_dir: PathBuf::from("s01-rootfs-source"),
        parent_stage,
        add_plan: ProducerPlan {
            source_rootfs_dir: None,
            producers: add_producers,
        },
        required_services,
        rootfs_source_policy,
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

    let mut add_plan = spec.add_plan.clone();
    if add_plan.producers.iter().any(|producer| {
        matches!(
            producer,
            RootfsProducer::CopyTree { .. } | RootfsProducer::CopyFile { .. }
        )
    }) {
        let source_rootfs_dir = materialize_stage01_source_rootfs(spec, output_dir)?;
        add_plan.source_rootfs_dir = Some(source_rootfs_dir);
    }

    apply_producer_plan(&add_plan, &rootfs_source_dir).with_context(|| {
        format!(
            "applying Stage 01 additive producers for '{}'",
            spec.distro_id
        )
    })?;
    install_stage_test_scripts(&spec.repo_root, &rootfs_source_dir).with_context(|| {
        format!(
            "installing stage test scripts into Stage 01 rootfs for '{}'",
            spec.distro_id
        )
    })?;
    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_stage01_shell(&rootfs_source_dir, &spec.os_name, inittab).with_context(
            || {
                format!(
                    "ensuring OpenRC Stage 01 serial shell for '{}'",
                    spec.distro_id
                )
            },
        )?;
    }
    if matches!(&spec.overlay, S01OverlayPolicy::Systemd { .. }) {
        ensure_systemd_stage01_default_target(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 default target for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_stage01_sshd_dirs(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 sshd directories for '{}'",
                spec.distro_id
            )
        })?;
        ensure_systemd_stage01_locale_completeness(&rootfs_source_dir).with_context(|| {
            format!(
                "ensuring systemd Stage 01 locale completeness for '{}'",
                spec.distro_id
            )
        })?;
    }

    let stage_issue_banner = stage_issue_banner(&spec.os_name, "S01 Boot");
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
        rename_live_overlay_for_stage(output_dir, &live_overlay_dir, STAGE01_ARTIFACT_TAG)
            .with_context(|| {
                format!(
                    "renaming Stage 01 live overlay directory for '{}'",
                    spec.distro_id
                )
            })?;

    if let S01OverlayPolicy::OpenRc { inittab, .. } = spec.overlay {
        ensure_openrc_stage01_shell(&live_overlay_dir, &spec.os_name, inittab).with_context(
            || {
                format!(
                    "ensuring OpenRC Stage 01 serial shell for '{}'",
                    spec.distro_id
                )
            },
        )?;
    }
    ensure_stage01_required_service_wiring(
        &live_overlay_dir,
        &spec.overlay,
        &spec.required_services,
    )
    .with_context(|| {
        format!(
            "ensuring Stage 01 required service wiring for '{}'",
            spec.distro_id
        )
    })?;

    Ok(S01BootInputs {
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

fn ensure_systemd_stage01_default_target(rootfs_dir: &Path) -> Result<()> {
    let default_target = rootfs_dir.join("etc/systemd/system/default.target");
    if let Some(parent) = default_target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    if default_target.exists() || default_target.symlink_metadata().is_ok() {
        fs::remove_file(&default_target)
            .with_context(|| format!("removing '{}'", default_target.display()))?;
    }
    symlink("/usr/lib/systemd/system/multi-user.target", &default_target).with_context(|| {
        format!(
            "linking '{}' -> '/usr/lib/systemd/system/multi-user.target'",
            default_target.display()
        )
    })?;
    Ok(())
}

fn ensure_systemd_stage01_sshd_dirs(rootfs_dir: &Path) -> Result<()> {
    for rel in ["var/empty/sshd", "usr/share/empty.sshd"] {
        let privsep_dir = rootfs_dir.join(rel);
        fs::create_dir_all(&privsep_dir)
            .with_context(|| format!("creating '{}'", privsep_dir.display()))?;
        let mut perms = fs::metadata(&privsep_dir)
            .with_context(|| format!("reading metadata '{}'", privsep_dir.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&privsep_dir, perms)
            .with_context(|| format!("setting permissions '{}'", privsep_dir.display()))?;
    }

    // Some Stage 01 payloads provide only sshd_config.anaconda.
    // Ensure canonical sshd_config exists for sshd.service.
    let ssh_dir = rootfs_dir.join("etc/ssh");
    let sshd_config = ssh_dir.join("sshd_config");
    if !sshd_config.is_file() {
        let anaconda_config = ssh_dir.join("sshd_config.anaconda");
        if anaconda_config.is_file() {
            fs::copy(&anaconda_config, &sshd_config).with_context(|| {
                format!(
                    "copying fallback sshd config '{}' -> '{}'",
                    anaconda_config.display(),
                    sshd_config.display()
                )
            })?;
        } else {
            fs::create_dir_all(&ssh_dir)
                .with_context(|| format!("creating '{}'", ssh_dir.display()))?;
            fs::write(
                &sshd_config,
                "PermitRootLogin yes\nPasswordAuthentication yes\nUsePAM yes\nInclude /etc/ssh/sshd_config.d/*.conf\n",
            )
            .with_context(|| format!("writing fallback sshd config '{}'", sshd_config.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn ensure_systemd_stage01_locale_completeness(rootfs_dir: &Path) -> Result<()> {
    let locale_payload_candidates = [
        "lib/locale/C.utf8/LC_CTYPE",
        "usr/lib/locale/C.utf8/LC_CTYPE",
        "lib64/locale/C.utf8/LC_CTYPE",
        "usr/lib64/locale/C.utf8/LC_CTYPE",
    ];
    let has_utf8_payload = locale_payload_candidates
        .iter()
        .any(|rel| rootfs_dir.join(rel).is_file());
    if !has_utf8_payload {
        bail!(
            "missing UTF-8 locale payload in Stage systemd rootfs '{}'; expected one of: {}",
            rootfs_dir.display(),
            locale_payload_candidates.join(", ")
        );
    }

    // Legacy-compatible canonical locale path: many consumers resolve locale
    // payload from /usr/lib/locale, while some upstream payloads ship it under
    // /lib/locale. Keep a single source of truth and expose it canonically.
    let lib_locale = rootfs_dir.join("lib/locale");
    let usr_lib_locale = rootfs_dir.join("usr/lib/locale");
    if lib_locale.is_dir() && !usr_lib_locale.exists() {
        if let Some(parent) = usr_lib_locale.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating '{}'", parent.display()))?;
        }
        symlink("/lib/locale", &usr_lib_locale)
            .with_context(|| format!("linking '{}' -> '/lib/locale'", usr_lib_locale.display()))?;
    }

    let etc_dir = rootfs_dir.join("etc");
    fs::create_dir_all(&etc_dir).with_context(|| format!("creating '{}'", etc_dir.display()))?;
    fs::write(etc_dir.join("locale.conf"), "LANG=C.UTF-8\n").with_context(|| {
        format!(
            "writing canonical locale config '{}'",
            etc_dir.join("locale.conf").display()
        )
    })?;
    Ok(())
}

pub(crate) fn ensure_stage01_required_service_wiring(
    live_overlay_dir: &Path,
    overlay_policy: &S01OverlayPolicy,
    required_services: &[String],
) -> Result<()> {
    for service in required_services {
        match (overlay_policy, service.as_str()) {
            (S01OverlayPolicy::Systemd { .. }, "sshd") => {
                let wants_dir = live_overlay_dir.join("etc/systemd/system/multi-user.target.wants");
                fs::create_dir_all(&wants_dir)
                    .with_context(|| format!("creating '{}'", wants_dir.display()))?;
                let wants_link = wants_dir.join("sshd.service");
                if wants_link.symlink_metadata().is_ok() {
                    fs::remove_file(&wants_link)
                        .with_context(|| format!("removing '{}'", wants_link.display()))?;
                }
                symlink("/usr/lib/systemd/system/sshd.service", &wants_link).with_context(
                    || {
                        format!(
                            "linking '{}' -> '/usr/lib/systemd/system/sshd.service'",
                            wants_link.display()
                        )
                    },
                )?;
            }
            (S01OverlayPolicy::OpenRc { .. }, "sshd") => {
                let runlevel_dir = live_overlay_dir.join("etc/runlevels/default");
                fs::create_dir_all(&runlevel_dir)
                    .with_context(|| format!("creating '{}'", runlevel_dir.display()))?;
                let service_link = runlevel_dir.join("sshd");
                if service_link.symlink_metadata().is_ok() {
                    fs::remove_file(&service_link)
                        .with_context(|| format!("removing '{}'", service_link.display()))?;
                }
                symlink("/etc/init.d/sshd", &service_link).with_context(|| {
                    format!("linking '{}' -> '/etc/init.d/sshd'", service_link.display())
                })?;
            }
            (_, other) => {
                bail!("unsupported Stage 01 required service '{}'", other);
            }
        }
    }
    Ok(())
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

fn parse_stage01_rootfs_source_policy(
    repo_root: &Path,
    config_path: &Path,
    source: Option<S01RootfsSourceToml>,
) -> Result<Option<S01RootfsSourcePolicy>> {
    let Some(source) = source else {
        return Ok(None);
    };

    match source.kind.trim().to_ascii_lowercase().as_str() {
        "recipe_rocky" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            let iso_name = source.iso_name.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.iso_name is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let sha256 = source.sha256.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.sha256 is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let sha256_url = source.sha256_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.sha256_url is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            let torrent_url = source.torrent_url.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "invalid Stage 01 config '{}': rootfs_source.torrent_url is required for kind='recipe_rocky'",
                    config_path.display()
                )
            })?;
            Ok(Some(S01RootfsSourcePolicy::RecipeRocky {
                recipe_script,
                iso_name: iso_name.trim().to_string(),
                sha256: sha256.trim().to_string(),
                sha256_url: sha256_url.trim().to_string(),
                torrent_url: torrent_url.trim().to_string(),
            }))
        }
        "recipe_custom" => {
            let recipe_script = resolve_repo_path(repo_root, source.recipe_script.trim());
            Ok(Some(S01RootfsSourcePolicy::RecipeCustom {
                recipe_script,
                defines: source.defines.unwrap_or_default(),
            }))
        }
        other => bail!(
            "invalid Stage 01 config '{}': unsupported rootfs_source.kind '{}'",
            config_path.display(),
            other
        ),
    }
}

fn materialize_stage01_source_rootfs(
    spec: &S01BootInputSpec,
    output_dir: &Path,
) -> Result<PathBuf> {
    match &spec.rootfs_source_policy {
        Some(S01RootfsSourcePolicy::RecipeRocky {
            recipe_script,
            iso_name,
            sha256,
            sha256_url,
            torrent_url,
        }) => {
            let build_dir = output_dir.join("stage01-rootfs-provider/rocky");
            let work_downloads_dir = stage_work_downloads_dir(&spec.repo_root, &spec.distro_id)?;
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 Rocky source provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs(
                &spec.repo_root,
                &build_dir,
                &RockyStage01RecipeSpec {
                    recipe_script: recipe_script.clone(),
                    iso_name: iso_name.clone(),
                    sha256: sha256.clone(),
                    sha256_url: sha256_url.clone(),
                    torrent_url: torrent_url.clone(),
                    preseed_iso_path: work_downloads_dir.join(iso_name),
                    trust_dir: work_downloads_dir,
                },
            )
            .with_context(|| {
                format!(
                    "materializing Stage 01 Rocky source rootfs for '{}'",
                    spec.distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        Some(S01RootfsSourcePolicy::RecipeCustom {
            recipe_script,
            defines,
        }) => {
            let build_dir = output_dir.join("stage01-rootfs-provider/custom");
            fs::create_dir_all(&build_dir).with_context(|| {
                format!(
                    "creating Stage 01 custom source provider directory '{}'",
                    build_dir.display()
                )
            })?;
            let source_rootfs_dir = materialize_rootfs_from_recipe(
                &spec.repo_root,
                &build_dir,
                &Stage01RootfsRecipeSpec {
                    recipe_script: recipe_script.clone(),
                    defines: defines.clone(),
                },
            )
            .with_context(|| {
                format!(
                    "materializing Stage 01 custom source rootfs for '{}'",
                    spec.distro_id
                )
            })?;
            ensure_non_legacy_rootfs_source(&source_rootfs_dir)?;
            Ok(source_rootfs_dir)
        }
        None => bail!(
            "Stage 01 producer plan requires copy-based rootfs source, but no rootfs_source policy is configured for '{}'.",
            spec.distro_id
        ),
    }
}

fn stage_work_downloads_dir(repo_root: &Path, distro_id: &str) -> Result<PathBuf> {
    let normalized = match distro_id.trim().to_ascii_lowercase().as_str() {
        "levitate" | "leviso" => "levitate",
        "acorn" | "acornos" => "acorn",
        "iuppiter" | "iuppiteros" => "iuppiter",
        "ralph" | "ralphos" => "ralph",
        other => {
            bail!(
                "unsupported distro '{}' for Stage 01 work downloads directory resolution",
                other
            )
        }
    };
    let downloads = repo_root
        .join(".artifacts/work")
        .join(normalized)
        .join("downloads");
    fs::create_dir_all(&downloads).with_context(|| {
        format!(
            "creating Stage 01 work downloads directory '{}'",
            downloads.display()
        )
    })?;
    Ok(downloads)
}

fn stage00_baseline_producers(distro_id: &str, os_name: &str, os_id: &str) -> Vec<RootfsProducer> {
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
            content: os_release.clone(),
            mode: None,
        },
        // Keep only etc/os-release in Stage 00 rootfs source.
        // usr/lib/os-release is not required by current contract checks.
    ]
}

fn stage01_baseline_producers(overlay_kind: &str) -> Vec<RootfsProducer> {
    if overlay_kind == "systemd" {
        return vec![
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
        ParentStage::S00Build => {
            let s00_root = distro_output.join("s00-build");
            let run_id = latest_successful_stage_run_id(&s00_root)?.ok_or_else(|| {
                anyhow::anyhow!(
                    "missing successful Stage 00 run metadata under '{}'; build parent stage first",
                    s00_root.display()
                )
            })?;
            s00_root.join(run_id).join("s00-filesystem.erofs")
        }
    };
    if !path.is_file() {
        bail!(
            "missing parent stage rootfs image '{}'; build parent stage first",
            path.display()
        );
    }
    Ok(path)
}

fn latest_successful_stage_run_id(stage_root: &Path) -> Result<Option<String>> {
    if !stage_root.is_dir() {
        return Ok(None);
    }

    let mut runs: Vec<StageRunMetadataFile> = Vec::new();
    for entry in fs::read_dir(stage_root)
        .with_context(|| format!("reading stage runs directory '{}'", stage_root.display()))?
    {
        let entry = entry.with_context(|| {
            format!("iterating stage runs directory '{}'", stage_root.display())
        })?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let Some(run_name) = run_dir.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        if run_name.starts_with('.') {
            continue;
        }
        let path = run_dir.join("run-manifest.json");
        if !path.is_file() {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("reading stage run metadata '{}'", path.display()))?;
        let parsed: StageRunMetadataFile = serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing stage run metadata '{}'", path.display()))?;
        if parsed.status == "success" {
            runs.push(parsed);
        }
    }

    runs.sort_by(|a, b| {
        let ak = a
            .finished_at_utc
            .clone()
            .unwrap_or_else(|| a.created_at_utc.clone());
        let bk = b
            .finished_at_utc
            .clone()
            .unwrap_or_else(|| b.created_at_utc.clone());
        bk.cmp(&ak)
    });

    Ok(runs.first().map(|r| r.run_id.clone()))
}

fn apply_producer_plan(plan: &ProducerPlan, destination_root: &Path) -> Result<()> {
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
            RootfsProducer::CopyTree { .. } | RootfsProducer::CopyFile { .. }
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

fn ensure_non_legacy_rootfs_source(path: &Path) -> Result<()> {
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

pub(crate) fn install_stage_test_scripts(repo_root: &Path, rootfs_source_dir: &Path) -> Result<()> {
    let scripts_src = repo_root.join("testing/install-tests/test-scripts");
    if !scripts_src.is_dir() {
        bail!(
            "stage test scripts source directory not found: '{}'",
            scripts_src.display()
        );
    }

    let bin_dst = rootfs_source_dir.join("usr/local/bin");
    let lib_dst = rootfs_source_dir.join("usr/local/lib/stage-tests");
    fs::create_dir_all(&bin_dst)
        .with_context(|| format!("creating stage scripts bin dir '{}'", bin_dst.display()))?;
    fs::create_dir_all(&lib_dst)
        .with_context(|| format!("creating stage scripts lib dir '{}'", lib_dst.display()))?;

    let entries = fs::read_dir(&scripts_src)
        .with_context(|| format!("reading stage scripts dir '{}'", scripts_src.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("reading directory entry in '{}'", scripts_src.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for '{}'", entry.path().display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let source = entry.path();

        if file_type.is_file() && name.starts_with("stage-") && name.ends_with(".sh") {
            let dest = bin_dst.join(name.as_ref());
            fs::copy(&source, &dest).with_context(|| {
                format!(
                    "copying stage script '{}' to '{}'",
                    source.display(),
                    dest.display()
                )
            })?;
            let mut perms = fs::metadata(&dest)
                .with_context(|| format!("reading metadata '{}'", dest.display()))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)
                .with_context(|| format!("setting permissions '{}'", dest.display()))?;
        }
    }

    let common_src = scripts_src.join("lib/common.sh");
    if !common_src.is_file() {
        bail!(
            "stage test common library not found: '{}'",
            common_src.display()
        );
    }
    let common_dst = lib_dst.join("common.sh");
    fs::copy(&common_src, &common_dst).with_context(|| {
        format!(
            "copying stage test common library '{}' to '{}'",
            common_src.display(),
            common_dst.display()
        )
    })?;
    let mut perms = fs::metadata(&common_dst)
        .with_context(|| format!("reading metadata '{}'", common_dst.display()))?
        .permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&common_dst, perms)
        .with_context(|| format!("setting permissions '{}'", common_dst.display()))?;

    Ok(())
}

fn create_empty_overlay(output_dir: &Path) -> Result<PathBuf> {
    let live_overlay = output_dir.join(format!("{STAGE00_ARTIFACT_TAG}-live-overlay"));
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

fn ensure_openrc_stage01_shell(
    rootfs_source_dir: &Path,
    os_name: &str,
    inittab: InittabVariant,
) -> Result<()> {
    let etc_dir = rootfs_source_dir.join("etc");
    let usr_local_bin = rootfs_source_dir.join("usr/local/bin");
    fs::create_dir_all(&etc_dir)
        .with_context(|| format!("creating OpenRC etc dir '{}'", etc_dir.display()))?;
    fs::create_dir_all(&usr_local_bin).with_context(|| {
        format!(
            "creating OpenRC usr/local/bin dir '{}'",
            usr_local_bin.display()
        )
    })?;

    let autologin = usr_local_bin.join("serial-autologin");
    fs::write(
        &autologin,
        "#!/bin/sh\necho \"___SHELL_READY___\"\necho \"___PROMPT___\"\necho \"___SHELL_READY___\" >/dev/console 2>/dev/null || true\necho \"___PROMPT___\" >/dev/console 2>/dev/null || true\necho \"___SHELL_READY___\" >/dev/kmsg 2>/dev/null || true\necho \"___PROMPT___\" >/dev/kmsg 2>/dev/null || true\nexec /bin/sh -l\n",
    )
    .with_context(|| format!("writing '{}'", autologin.display()))?;
    let mut perms = fs::metadata(&autologin)
        .with_context(|| format!("reading metadata '{}'", autologin.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&autologin, perms)
        .with_context(|| format!("setting permissions on '{}'", autologin.display()))?;

    let inittab_content = match inittab {
        InittabVariant::DesktopWithSerial => format!(
            r#"# /etc/inittab - {os_name} Live
# Stage 01 boots to minimal interactive shell.
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100
::wait:/sbin/openrc default
::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
"#
        ),
        InittabVariant::SerialOnly => format!(
            r#"# /etc/inittab - {os_name} Live
# Stage 01 boots to minimal interactive shell.
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100
::wait:/sbin/openrc default
::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
"#
        ),
    };
    let inittab_path = etc_dir.join("inittab");
    fs::write(&inittab_path, inittab_content)
        .with_context(|| format!("writing '{}'", inittab_path.display()))?;

    let issue_path = etc_dir.join("issue");
    fs::write(
        &issue_path,
        format!(
            "\n{} S01 Boot Live - \\l\n\nLogin as 'root' (no password)\n\n",
            os_name
        ),
    )
    .with_context(|| format!("writing '{}'", issue_path.display()))?;

    let shadow_path = etc_dir.join("shadow");
    fs::write(
        &shadow_path,
        "root::0:0:99999:7:::\nbin:!:0:0:99999:7:::\ndaemon:!:0:0:99999:7:::\nnobody:!:0:0:99999:7:::\n",
    )
    .with_context(|| format!("writing '{}'", shadow_path.display()))?;
    let mut shadow_perms = fs::metadata(&shadow_path)
        .with_context(|| format!("reading metadata '{}'", shadow_path.display()))?
        .permissions();
    shadow_perms.set_mode(0o640);
    fs::set_permissions(&shadow_path, shadow_perms)
        .with_context(|| format!("setting permissions on '{}'", shadow_path.display()))?;

    Ok(())
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
        let producers = stage00_baseline_producers("levitate", "LevitateOS", "levitateos");
        let paths: Vec<PathBuf> = producers
            .iter()
            .filter_map(|p| match p {
                RootfsProducer::WriteText { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        assert!(paths.contains(&PathBuf::from("etc/os-release")));
        assert!(paths.contains(&PathBuf::from("usr/lib/stage-manifest.json")));
    }

    #[test]
    fn legacy_rootfs_source_is_rejected() {
        let mut legacy = PathBuf::from("/data/vince/LevitateOS");
        for component in ["leviso", "downloads", "rootfs"] {
            legacy.push(component);
        }
        let result = ensure_non_legacy_rootfs_source(&legacy);
        assert!(result.is_err(), "legacy rootfs path must fail policy");
    }

    #[test]
    fn missing_rootfs_source_filters_copy_producers() {
        let policy = parse_stage01_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/acorn/01Boot.toml"),
            None,
        )
        .expect("parsing missing rootfs_source must be allowed");
        assert!(
            policy.is_none(),
            "missing Stage 01 rootfs_source should remain optional"
        );

        let mut producers = stage01_baseline_producers("openrc");
        if policy.is_none() {
            producers.retain(|producer| matches!(producer, RootfsProducer::WriteText { .. }));
        }

        assert!(!producers.is_empty());
        assert!(producers
            .iter()
            .all(|producer| matches!(producer, RootfsProducer::WriteText { .. })));
    }

    #[test]
    fn rootfs_source_policy_accepts_custom_recipe_for_any_distro() {
        let source = S01RootfsSourceToml {
            kind: "recipe_custom".to_string(),
            recipe_script: "distro-builder/recipes/custom-stage01-rootfs.rhai".to_string(),
            iso_name: None,
            sha256: None,
            sha256_url: None,
            torrent_url: None,
            defines: Some(BTreeMap::from([(
                "CUSTOM_ROOTFS_DIR".to_string(),
                "/tmp/rootfs".to_string(),
            )])),
        };
        let policy = parse_stage01_rootfs_source_policy(
            Path::new("."),
            &PathBuf::from("distro-variants/acorn/01Boot.toml"),
            Some(source),
        )
        .expect("parsing custom rootfs_source policy must succeed");

        assert!(matches!(
            policy,
            Some(S01RootfsSourcePolicy::RecipeCustom { .. })
        ));
    }

    #[test]
    fn stage_scoped_rootfs_source_is_allowed() {
        let stage_scoped = Path::new(
            "/data/vince/LevitateOS/.artifacts/out/levitate/s01-boot/s01-rootfs-source-12345-12345",
        );
        let result = ensure_non_legacy_rootfs_source(stage_scoped);
        assert!(
            result.is_ok(),
            "stage-scoped rootfs path should be accepted"
        );
    }

    #[test]
    fn stage_work_downloads_dir_normalizes_aliases() {
        let unique = format!(
            "levitateos-s01-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("create temp repo root");

        let downloads =
            stage_work_downloads_dir(&repo_root, "leviso").expect("resolve alias downloads dir");
        assert!(
            downloads.ends_with(".artifacts/work/levitate/downloads"),
            "expected normalized levitate work downloads path, got {}",
            downloads.display()
        );
    }

    #[test]
    fn stage_work_downloads_dir_rejects_unknown_distro() {
        let unique = format!(
            "levitateos-s01-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let repo_root = std::env::temp_dir().join(unique);
        fs::create_dir_all(&repo_root).expect("create temp repo root");

        let result = stage_work_downloads_dir(&repo_root, "unknown");
        assert!(result.is_err(), "unknown distro should fail");
    }
}
