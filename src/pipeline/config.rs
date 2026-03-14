use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::pipeline::live_tools::{InstallDocsFrontend, InstallExperience, LiveToolsRuntimeAction};
use crate::pipeline::overlay::S01OverlayPolicy;
use crate::pipeline::paths::resolve_repo_path;
use crate::pipeline::plan::RootfsProducer;
use crate::pipeline::source::{
    load_rootfs_source_policy, S01RootfsSourcePolicy, S01RootfsSourceToml,
};
use crate::InittabVariant;

#[derive(Debug, Clone)]
pub(crate) struct S01LoadedConfig {
    pub(crate) os_name: String,
    pub(crate) required_services: Vec<String>,
    pub(crate) rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub(crate) overlay: S01OverlayPolicy,
    pub(crate) payload_producers: Vec<RootfsProducer>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveToolsLoadedConfig {
    pub(crate) os_name: String,
    pub(crate) install_experience: InstallExperience,
    pub(crate) runtime_actions: Vec<LiveToolsRuntimeAction>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2ProductsToml {
    #[allow(dead_code)]
    schema_version: u32,
    ring2_products: Ring2ProductsSectionToml,
    ring2_payload_profiles: Option<BTreeMap<String, Ring2PayloadProfileToml>>,
    ring2_runtime_profiles: Option<BTreeMap<String, Ring2RuntimeProfileToml>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2ProductsSectionToml {
    #[allow(dead_code)]
    rootfs_base: Ring2ProductDeclToml,
    live_overlay: Ring2LiveOverlayToml,
    #[allow(dead_code)]
    boot_live: Ring2ProductDeclToml,
    #[allow(dead_code)]
    live_tools: Ring2ProductDeclToml,
    #[allow(dead_code)]
    boot_installed: Option<Ring2ProductDeclToml>,
    #[allow(dead_code)]
    kernel_staging: Ring2ProductDeclToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2ProductDeclToml {
    #[allow(dead_code)]
    logical_name: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    extends: Option<String>,
    payload_profile: Option<String>,
    runtime_profiles: Option<Vec<String>>,
    runtime_profiles_ux: Option<Vec<String>>,
    runtime_profiles_automated_ssh: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2LiveOverlayToml {
    #[allow(dead_code)]
    logical_name: String,
    #[allow(dead_code)]
    description: String,
    #[allow(dead_code)]
    extends: Option<String>,
    overlay_kind: String,
    issue_message: Option<String>,
    openrc_inittab: Option<String>,
    profile_overlay: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2PayloadProfileToml {
    producers: Vec<Ring2RootfsProducerToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ring2RuntimeProfileToml {
    actions: Vec<Ring2RuntimeActionToml>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Ring2RuntimeActionToml {
    ToolPayloadWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
    },
    RootfsWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
        destination: String,
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

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Ring2RootfsProducerToml {
    CopyTree {
        source: String,
        destination: String,
    },
    CopySymlink {
        source: String,
        destination: String,
    },
    CopyFile {
        source: String,
        destination: String,
        #[serde(default)]
        optional: bool,
    },
    WriteText {
        path: String,
        content: String,
        mode: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy)]
enum BootPayloadProduct {
    Live,
    Installed,
}

impl BootPayloadProduct {
    fn logical_name(self) -> &'static str {
        match self {
            Self::Live => "product.payload.boot.live",
            Self::Installed => "product.payload.boot.installed",
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentityToml {
    #[allow(dead_code)]
    schema_version: u32,
    identity: IdentitySectionToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IdentitySectionToml {
    os_name: String,
    #[allow(dead_code)]
    os_id: String,
    #[allow(dead_code)]
    iso_label: String,
    #[allow(dead_code)]
    os_version: String,
    #[allow(dead_code)]
    default_hostname: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenariosToml {
    #[allow(dead_code)]
    schema_version: u32,
    scenarios: ScenarioSectionsToml,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioSectionsToml {
    live_boot: Option<LiveBootScenarioToml>,
    live_environment: Option<LiveEnvironmentScenarioToml>,
    #[allow(dead_code)]
    live_tools: Option<LiveToolsScenarioToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveBootScenarioToml {
    #[allow(dead_code)]
    required_kernel_cmdline: Vec<String>,
    required_live_services: Vec<String>,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveEnvironmentScenarioToml {
    required_services: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveToolsScenarioToml {
    install_experience: InstallExperience,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioEvidenceToml {
    #[allow(dead_code)]
    script_path: String,
    #[allow(dead_code)]
    pass_marker: String,
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
    os_name: Option<String>,
    overlay_kind: Option<String>,
    required_services: Option<Vec<String>>,
    rootfs_source: Option<S01RootfsSourceToml>,
    openrc_inittab: Option<String>,
    profile_overlay: Option<String>,
    issue_message: Option<String>,
}

pub(crate) fn load_boot_payload_config(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01LoadedConfig> {
    let config_path = variant_dir.join("01Boot.toml");
    let legacy_boot_inputs = load_legacy_boot_inputs(&config_path)?;

    let os_name = load_identity_os_name(variant_dir, legacy_boot_inputs.as_ref())?;
    let required_services = load_required_services(variant_dir, legacy_boot_inputs.as_ref())?;
    let overlay = load_ring2_overlay_policy(
        repo_root,
        variant_dir,
        &config_path,
        distro_id,
        legacy_boot_inputs.as_ref(),
    )?;
    let payload_producers =
        load_boot_payload_producers(variant_dir, BootPayloadProduct::Live, &overlay)?;

    let rootfs_source_policy = load_rootfs_source_policy(
        repo_root,
        variant_dir,
        &config_path,
        legacy_boot_inputs
            .as_ref()
            .and_then(|inputs| inputs.rootfs_source.clone()),
    )?;

    Ok(S01LoadedConfig {
        os_name,
        required_services,
        rootfs_source_policy,
        overlay,
        payload_producers,
    })
}

pub(crate) fn load_boot_config(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01LoadedConfig> {
    let loaded = load_boot_payload_config(repo_root, variant_dir, distro_id)?;
    if !loaded.required_services.iter().any(|svc| svc == "sshd") {
        bail!(
            "invalid live boot config for '{}': required_services must include 'sshd' (OpenSSH is first-class in live boot)",
            distro_id
        );
    }
    Ok(loaded)
}

pub(crate) fn load_installed_boot_payload_config(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01LoadedConfig> {
    let mut loaded = load_boot_payload_config(repo_root, variant_dir, distro_id)?;
    loaded.payload_producers =
        load_boot_payload_producers(variant_dir, BootPayloadProduct::Installed, &loaded.overlay)?;
    Ok(loaded)
}

pub(crate) fn load_live_tools_config(variant_dir: &Path) -> Result<LiveToolsLoadedConfig> {
    let os_name = load_live_tools_os_name(variant_dir)?;
    let install_experience = load_live_tools_install_experience(variant_dir)?;
    let runtime_actions = load_live_tools_runtime_actions(variant_dir, install_experience)?;

    Ok(LiveToolsLoadedConfig {
        os_name,
        install_experience,
        runtime_actions,
    })
}

fn load_ring2_products_manifest(variant_dir: &Path) -> Result<Option<Ring2ProductsToml>> {
    let ring2_config_path = variant_dir.join("ring2-products.toml");
    if !ring2_config_path.is_file() {
        return Ok(None);
    }

    let config_bytes = fs::read_to_string(&ring2_config_path).with_context(|| {
        format!(
            "reading Ring 2 product config '{}'",
            ring2_config_path.display()
        )
    })?;
    let parsed: Ring2ProductsToml = toml::from_str(&config_bytes).with_context(|| {
        format!(
            "parsing Ring 2 product config '{}'",
            ring2_config_path.display()
        )
    })?;
    Ok(Some(parsed))
}

fn load_boot_payload_producers(
    variant_dir: &Path,
    product: BootPayloadProduct,
    _overlay: &S01OverlayPolicy,
) -> Result<Vec<RootfsProducer>> {
    let manifest_path = variant_dir.join("ring2-products.toml");
    let parsed = load_ring2_products_manifest(variant_dir)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical Ring 2 boot payload owner '{}' for '{}': expected '{}'",
            product.logical_name(),
            variant_dir.display(),
            manifest_path.display()
        )
    })?;

    let decl =
        match product {
            BootPayloadProduct::Live => &parsed.ring2_products.boot_live,
            BootPayloadProduct::Installed => parsed
                .ring2_products
                .boot_installed
                .as_ref()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "missing canonical Ring 2 product declaration '{}' in '{}'",
                        product.logical_name(),
                        variant_dir.join("ring2-products.toml").display()
                    )
                })?,
        };

    let profile_name = decl.payload_profile.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical Ring 2 payload_profile for '{}' in '{}'",
            product.logical_name(),
            variant_dir.join("ring2-products.toml").display()
        )
    })?;

    let profiles = parsed.ring2_payload_profiles.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "missing ring2_payload_profiles section in '{}'; required by payload_profile '{}'",
            variant_dir.join("ring2-products.toml").display(),
            profile_name
        )
    })?;

    let profile = profiles.get(profile_name).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown Ring 2 payload profile '{}' referenced by '{}' in '{}'",
            profile_name,
            product.logical_name(),
            variant_dir.join("ring2-products.toml").display()
        )
    })?;

    if profile.producers.is_empty() {
        bail!(
            "Ring 2 payload profile '{}' in '{}' must declare at least one producer",
            profile_name,
            variant_dir.join("ring2-products.toml").display()
        );
    }

    profile
        .producers
        .iter()
        .map(rootfs_producer_from_ring2_toml)
        .collect()
}

fn load_live_tools_runtime_actions(
    variant_dir: &Path,
    install_experience: InstallExperience,
) -> Result<Vec<LiveToolsRuntimeAction>> {
    let manifest_path = variant_dir.join("ring2-products.toml");
    let parsed = load_ring2_products_manifest(variant_dir)?.ok_or_else(|| {
        anyhow::anyhow!(
            "missing canonical Ring 2 live-tools owner for '{}': expected '{}'",
            variant_dir.display(),
            manifest_path.display()
        )
    })?;

    let live_tools = &parsed.ring2_products.live_tools;
    let mut profile_names = live_tools.runtime_profiles.clone().unwrap_or_default();
    match install_experience {
        InstallExperience::Ux => {
            profile_names.extend(live_tools.runtime_profiles_ux.clone().unwrap_or_default());
        }
        InstallExperience::AutomatedSsh => {
            profile_names.extend(
                live_tools
                    .runtime_profiles_automated_ssh
                    .clone()
                    .unwrap_or_default(),
            );
        }
    }

    if profile_names.is_empty() {
        bail!(
            "missing canonical Ring 2 runtime profile selection for '{}' in '{}': declare runtime_profiles and runtime_profiles_* on [ring2_products.live_tools]",
            parsed.ring2_products.live_tools.logical_name,
            manifest_path.display()
        );
    }

    let profiles = parsed.ring2_runtime_profiles.as_ref().ok_or_else(|| {
        anyhow::anyhow!(
            "missing ring2_runtime_profiles section in '{}'; required by live-tools runtime_profiles",
            variant_dir.join("ring2-products.toml").display()
        )
    })?;

    let mut actions = Vec::new();
    for profile_name in &profile_names {
        let profile = profiles.get(profile_name).ok_or_else(|| {
            anyhow::anyhow!(
                "unknown Ring 2 runtime profile '{}' referenced by '{}' in '{}'",
                profile_name,
                parsed.ring2_products.live_tools.logical_name,
                variant_dir.join("ring2-products.toml").display()
            )
        })?;
        if profile.actions.is_empty() {
            bail!(
                "Ring 2 runtime profile '{}' in '{}' must declare at least one action",
                profile_name,
                variant_dir.join("ring2-products.toml").display()
            );
        }
        for action in &profile.actions {
            actions.push(live_tools_runtime_action_from_toml(action)?);
        }
    }

    Ok(actions)
}

fn live_tools_runtime_action_from_toml(
    action: &Ring2RuntimeActionToml,
) -> Result<LiveToolsRuntimeAction> {
    fn normalize_string(raw: &str, field: &str) -> Result<String> {
        let value = raw.trim();
        if value.is_empty() {
            bail!("Ring 2 runtime action field '{}' must not be empty", field);
        }
        Ok(value.to_string())
    }

    fn normalize_relative_path(raw: &str, field: &str) -> Result<String> {
        let value = normalize_string(raw, field)?;
        let path = std::path::Path::new(&value);
        if path.is_absolute() {
            bail!(
                "Ring 2 runtime action field '{}' must be relative, got '{}'",
                field,
                path.display()
            );
        }
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!(
                "Ring 2 runtime action field '{}' must not traverse parents, got '{}'",
                field,
                path.display()
            );
        }
        Ok(value)
    }

    Ok(match action {
        Ring2RuntimeActionToml::ToolPayloadWorkspaceBinary {
            package,
            binary,
            target,
        } => LiveToolsRuntimeAction::ToolPayloadWorkspaceBinary {
            package: normalize_string(package, "package")?,
            binary: binary
                .as_deref()
                .map(|value| normalize_string(value, "binary"))
                .transpose()?,
            target: target
                .as_deref()
                .map(|value| normalize_string(value, "target"))
                .transpose()?,
        },
        Ring2RuntimeActionToml::RootfsWorkspaceBinary {
            package,
            binary,
            target,
            destination,
        } => LiveToolsRuntimeAction::RootfsWorkspaceBinary {
            package: normalize_string(package, "package")?,
            binary: binary
                .as_deref()
                .map(|value| normalize_string(value, "binary"))
                .transpose()?,
            target: target
                .as_deref()
                .map(|value| normalize_string(value, "target"))
                .transpose()?,
            destination: std::path::PathBuf::from(normalize_relative_path(
                destination,
                "destination",
            )?),
        },
        Ring2RuntimeActionToml::ApkPackages { packages } => {
            let packages = packages
                .iter()
                .map(|package| normalize_string(package, "packages"))
                .collect::<Result<Vec<_>>>()?;
            if packages.is_empty() {
                bail!("Ring 2 runtime action 'apk_packages' must declare at least one package");
            }
            LiveToolsRuntimeAction::ApkPackages { packages }
        }
        Ring2RuntimeActionToml::IuppiterDarPayload { target } => {
            LiveToolsRuntimeAction::IuppiterDarPayload {
                target: target
                    .as_deref()
                    .map(|value| normalize_string(value, "target"))
                    .transpose()?,
            }
        }
        Ring2RuntimeActionToml::InstallModePayload {
            interactive_shell,
            ux_docs_frontend,
        } => {
            let interactive_shell = normalize_string(interactive_shell, "interactive_shell")?;
            let shell_path = std::path::Path::new(&interactive_shell);
            if !shell_path.is_absolute() {
                bail!(
                    "Ring 2 runtime action field 'interactive_shell' must be absolute, got '{}'",
                    shell_path.display()
                );
            }
            if shell_path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                bail!(
                    "Ring 2 runtime action field 'interactive_shell' must not traverse parents, got '{}'",
                    shell_path.display()
                );
            }
            LiveToolsRuntimeAction::InstallModePayload {
                interactive_shell,
                ux_docs_frontend: *ux_docs_frontend,
            }
        }
    })
}

fn rootfs_producer_from_ring2_toml(producer: &Ring2RootfsProducerToml) -> Result<RootfsProducer> {
    fn normalized_relative_path(raw: &str, field: &str) -> Result<std::path::PathBuf> {
        let path = std::path::PathBuf::from(raw.trim());
        if raw.trim().is_empty() {
            bail!(
                "Ring 2 payload producer field '{}' must not be empty",
                field
            );
        }
        if path.is_absolute() {
            bail!(
                "Ring 2 payload producer field '{}' must be relative, got '{}'",
                field,
                path.display()
            );
        }
        if path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!(
                "Ring 2 payload producer field '{}' must not traverse parents, got '{}'",
                field,
                path.display()
            );
        }
        Ok(path)
    }

    Ok(match producer {
        Ring2RootfsProducerToml::CopyTree {
            source,
            destination,
        } => RootfsProducer::CopyTree {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
        },
        Ring2RootfsProducerToml::CopySymlink {
            source,
            destination,
        } => RootfsProducer::CopySymlink {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
        },
        Ring2RootfsProducerToml::CopyFile {
            source,
            destination,
            optional,
        } => RootfsProducer::CopyFile {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
            optional: *optional,
        },
        Ring2RootfsProducerToml::WriteText {
            path,
            content,
            mode,
        } => RootfsProducer::WriteText {
            path: normalized_relative_path(path, "path")?,
            content: content.clone(),
            mode: *mode,
        },
    })
}

fn load_legacy_boot_inputs(config_path: &Path) -> Result<Option<S01BootInputsToml>> {
    if !config_path.is_file() {
        return Ok(None);
    }

    let config_bytes = fs::read_to_string(config_path)
        .with_context(|| format!("reading Stage 01 config '{}'", config_path.display()))?;
    let parsed: S01BootToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing Stage 01 config '{}'", config_path.display()))?;
    Ok(Some(parsed.stage_01.boot_inputs))
}

fn load_identity_os_name(
    variant_dir: &Path,
    legacy_boot_inputs: Option<&S01BootInputsToml>,
) -> Result<String> {
    let identity_path = variant_dir.join("identity.toml");
    let ring_os_name = if identity_path.is_file() {
        let config_bytes = fs::read_to_string(&identity_path)
            .with_context(|| format!("reading identity config '{}'", identity_path.display()))?;
        let parsed: IdentityToml = toml::from_str(&config_bytes)
            .with_context(|| format!("parsing identity config '{}'", identity_path.display()))?;
        Some(parsed.identity.os_name)
    } else {
        None
    };

    let legacy_os_name = legacy_boot_inputs
        .and_then(|inputs| inputs.os_name.as_ref())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if let (Some(ring_os_name), Some(legacy_os_name)) = (&ring_os_name, &legacy_os_name) {
        if ring_os_name != legacy_os_name {
            bail!(
                "identity/base-product parity mismatch for '{}': legacy 01Boot os_name '{}' does not match identity.toml os_name '{}'",
                variant_dir.display(),
                legacy_os_name,
                ring_os_name
            );
        }
    }

    ring_os_name
        .or(legacy_os_name)
        .ok_or_else(|| anyhow::anyhow!("missing canonical os_name for '{}'", variant_dir.display()))
}

fn load_live_tools_os_name(variant_dir: &Path) -> Result<String> {
    let identity_path = variant_dir.join("identity.toml");
    if !identity_path.is_file() {
        bail!(
            "missing canonical live-tools identity owner for '{}': expected '{}'",
            variant_dir.display(),
            identity_path.display()
        );
    }

    let config_bytes = fs::read_to_string(&identity_path)
        .with_context(|| format!("reading identity config '{}'", identity_path.display()))?;
    let parsed: IdentityToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing identity config '{}'", identity_path.display()))?;
    let os_name = parsed.identity.os_name.trim().to_string();
    if os_name.is_empty() {
        bail!(
            "invalid canonical live-tools identity owner '{}': identity.os_name must not be empty",
            identity_path.display()
        );
    }

    Ok(os_name)
}

fn load_required_services(
    variant_dir: &Path,
    legacy_boot_inputs: Option<&S01BootInputsToml>,
) -> Result<Vec<String>> {
    let scenarios_path = variant_dir.join("scenarios.toml");
    let ring_required_services = if scenarios_path.is_file() {
        let config_bytes = fs::read_to_string(&scenarios_path)
            .with_context(|| format!("reading scenarios config '{}'", scenarios_path.display()))?;
        let parsed: ScenariosToml = toml::from_str(&config_bytes)
            .with_context(|| format!("parsing scenarios config '{}'", scenarios_path.display()))?;
        let services = parsed
            .scenarios
            .live_environment
            .map(|env| env.required_services)
            .or_else(|| {
                parsed
                    .scenarios
                    .live_boot
                    .map(|boot| boot.required_live_services)
            });
        services.map(normalize_services)
    } else {
        None
    };

    let legacy_required_services = legacy_boot_inputs
        .and_then(|inputs| inputs.required_services.clone())
        .map(normalize_services);

    if let (Some(ring_required_services), Some(legacy_required_services)) =
        (&ring_required_services, &legacy_required_services)
    {
        if ring_required_services != legacy_required_services {
            bail!(
                "scenario/base-product parity mismatch for '{}': legacy 01Boot required_services {:?} does not match scenarios.toml required_services {:?}",
                variant_dir.display(),
                legacy_required_services,
                ring_required_services
            );
        }
    }

    Ok(ring_required_services
        .or(legacy_required_services)
        .unwrap_or_else(|| vec!["sshd".to_string()]))
}

fn load_live_tools_install_experience(variant_dir: &Path) -> Result<InstallExperience> {
    let scenarios_path = variant_dir.join("scenarios.toml");
    if !scenarios_path.is_file() {
        bail!(
            "missing canonical live-tools scenario owner for '{}': expected '{}'",
            variant_dir.display(),
            scenarios_path.display()
        );
    }

    let config_bytes = fs::read_to_string(&scenarios_path)
        .with_context(|| format!("reading scenarios config '{}'", scenarios_path.display()))?;
    let parsed: ScenariosToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing scenarios config '{}'", scenarios_path.display()))?;

    parsed
        .scenarios
        .live_tools
        .map(|tools| tools.install_experience)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical live-tools scenario owner '[scenarios.live_tools]' in '{}'",
                scenarios_path.display()
            )
        })
}

fn load_ring2_overlay_policy(
    repo_root: &Path,
    variant_dir: &Path,
    legacy_config_path: &Path,
    distro_id: &str,
    legacy_boot_inputs: Option<&S01BootInputsToml>,
) -> Result<S01OverlayPolicy> {
    let ring2_config_path = variant_dir.join("ring2-products.toml");
    let ring_overlay = if ring2_config_path.is_file() {
        let config_bytes = fs::read_to_string(&ring2_config_path).with_context(|| {
            format!(
                "reading Ring 2 product config '{}'",
                ring2_config_path.display()
            )
        })?;
        let parsed: Ring2ProductsToml = toml::from_str(&config_bytes).with_context(|| {
            format!(
                "parsing Ring 2 product config '{}'",
                ring2_config_path.display()
            )
        })?;
        Some(parsed.ring2_products.live_overlay)
    } else {
        None
    };

    if let (Some(ring_overlay), Some(legacy_inputs)) = (&ring_overlay, legacy_boot_inputs) {
        if let Some(legacy_overlay_kind) = legacy_inputs.overlay_kind.as_ref() {
            let legacy_overlay_kind = legacy_overlay_kind.trim().to_ascii_lowercase();
            let ring_overlay_kind = ring_overlay.overlay_kind.trim().to_ascii_lowercase();
            if ring_overlay_kind != legacy_overlay_kind {
                bail!(
                    "Ring 2 base-product parity mismatch between '{}' and '{}': legacy overlay_kind '{}' does not match ring2 overlay_kind '{}'",
                    legacy_config_path.display(),
                    ring2_config_path.display(),
                    legacy_overlay_kind,
                    ring_overlay_kind
                );
            }
        }

        if let Some(legacy_issue_message) = legacy_inputs.issue_message.as_ref() {
            if ring_overlay.issue_message.as_ref() != Some(legacy_issue_message) {
                bail!(
                    "Ring 2 base-product parity mismatch between '{}' and '{}': legacy issue_message {:?} does not match ring2 issue_message {:?}",
                    legacy_config_path.display(),
                    ring2_config_path.display(),
                    legacy_inputs.issue_message,
                    ring_overlay.issue_message
                );
            }
        }

        if let Some(legacy_inittab) = legacy_inputs.openrc_inittab.as_ref() {
            if ring_overlay.openrc_inittab.as_ref() != Some(legacy_inittab) {
                bail!(
                    "Ring 2 base-product parity mismatch between '{}' and '{}': legacy openrc_inittab {:?} does not match ring2 openrc_inittab {:?}",
                    legacy_config_path.display(),
                    ring2_config_path.display(),
                    legacy_inputs.openrc_inittab,
                    ring_overlay.openrc_inittab
                );
            }
        }

        if let Some(legacy_profile_overlay) = legacy_inputs.profile_overlay.as_ref() {
            if ring_overlay.profile_overlay.as_ref() != Some(legacy_profile_overlay) {
                bail!(
                    "Ring 2 base-product parity mismatch between '{}' and '{}': legacy profile_overlay {:?} does not match ring2 profile_overlay {:?}",
                    legacy_config_path.display(),
                    ring2_config_path.display(),
                    legacy_inputs.profile_overlay,
                    ring_overlay.profile_overlay
                );
            }
        }
    }

    match ring_overlay {
        Some(ring_overlay) => {
            overlay_policy_from_ring(repo_root, legacy_config_path, distro_id, ring_overlay)
        }
        None => {
            overlay_policy_from_legacy(repo_root, legacy_config_path, distro_id, legacy_boot_inputs)
        }
    }
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

fn overlay_policy_from_ring(
    repo_root: &Path,
    config_path: &Path,
    distro_id: &str,
    ring_overlay: Ring2LiveOverlayToml,
) -> Result<S01OverlayPolicy> {
    match ring_overlay.overlay_kind.trim().to_ascii_lowercase().as_str() {
        "systemd" => Ok(S01OverlayPolicy::Systemd {
            issue_message: ring_overlay.issue_message,
        }),
        "openrc" => {
            let inittab =
                parse_openrc_inittab(ring_overlay.openrc_inittab.as_deref(), config_path, distro_id)?;
            let profile_overlay = ring_overlay
                .profile_overlay
                .as_ref()
                .map(|path| resolve_repo_path(repo_root, path));
            Ok(S01OverlayPolicy::OpenRc {
                inittab,
                profile_overlay,
            })
        }
        other => bail!(
            "invalid Ring 2 config '{}': unsupported overlay_kind '{}' (expected 'systemd' or 'openrc')",
            config_path.display(),
            other
        ),
    }
}

fn overlay_policy_from_legacy(
    repo_root: &Path,
    config_path: &Path,
    distro_id: &str,
    legacy_boot_inputs: Option<&S01BootInputsToml>,
) -> Result<S01OverlayPolicy> {
    let Some(legacy_boot_inputs) = legacy_boot_inputs else {
        bail!(
            "missing canonical Ring 2 base-product owner for '{}': provide 'ring2-products.toml' or legacy '01Boot.toml'",
            distro_id
        );
    };
    let Some(legacy_overlay_kind) = legacy_boot_inputs.overlay_kind.as_ref() else {
        bail!(
            "invalid Stage 01 config '{}': overlay_kind is required when Ring 2 base-product config is absent",
            config_path.display()
        );
    };

    match legacy_overlay_kind.trim().to_ascii_lowercase().as_str() {
        "systemd" => Ok(S01OverlayPolicy::Systemd {
            issue_message: legacy_boot_inputs.issue_message.clone(),
        }),
        "openrc" => {
            let inittab = parse_openrc_inittab(
                legacy_boot_inputs.openrc_inittab.as_deref(),
                config_path,
                distro_id,
            )?;
            let profile_overlay = legacy_boot_inputs
                .profile_overlay
                .as_ref()
                .map(|path| resolve_repo_path(repo_root, path));
            Ok(S01OverlayPolicy::OpenRc {
                inittab,
                profile_overlay,
            })
        }
        other => bail!(
            "invalid Stage 01 config '{}': unsupported overlay_kind '{}' (expected 'systemd' or 'openrc')",
            config_path.display(),
            other
        ),
    }
}

fn normalize_services(raw_services: Vec<String>) -> Vec<String> {
    let mut required_services = raw_services
        .into_iter()
        .map(|service| service.trim().to_ascii_lowercase())
        .filter(|service| !service.is_empty())
        .collect::<Vec<_>>();
    required_services.sort();
    required_services.dedup();
    required_services
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::source::S01RootfsSourcePolicy;
    use std::path::Component;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    #[test]
    fn parse_relative_path_rejects_parent_traversal() {
        let result = parse_relative_path("../etc/passwd", "test");
        assert!(result.is_err());
    }

    fn assert_uses_fedora_stage01_recipes(repo_root: &Path, distro_id: &str) {
        let variant_dir = repo_root.join(format!("distro-variants/{distro_id}"));
        let loaded = load_boot_config(repo_root, &variant_dir, distro_id)
            .unwrap_or_else(|err| panic!("load {distro_id} 01Boot: {err:#}"));
        match loaded.rootfs_source_policy {
            Some(S01RootfsSourcePolicy::RecipeRpmDvd {
                recipe_script,
                preseed_recipe_script,
            }) => {
                assert!(
                    recipe_script.ends_with("distro-builder/recipes/fedora-stage01-rootfs.rhai"),
                    "unexpected Stage 01 recipe: {}",
                    recipe_script.display()
                );
                assert!(
                    preseed_recipe_script
                        .ends_with("distro-builder/recipes/fedora-preseed-iso.rhai"),
                    "unexpected Stage 01 preseed recipe: {}",
                    preseed_recipe_script.display()
                );
            }
            other => panic!("unexpected {distro_id} Stage 01 source policy: {other:?}"),
        }
    }

    #[test]
    fn levitate_boot_config_uses_fedora_stage01_recipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_fedora_stage01_recipes(&repo_root, "levitate");
    }

    #[test]
    fn ralph_boot_config_uses_fedora_stage01_recipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_fedora_stage01_recipes(&repo_root, "ralph");
    }

    fn temp_repo_root(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "distro-builder-config-{test_name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, content).expect("write file");
    }

    #[test]
    fn ring2_overlay_kind_matches_legacy_when_equal() {
        let repo_root = temp_repo_root("ring2-overlay-parity");
        let variant_dir = repo_root.join("distro-variants/levitate");
        let config_path = variant_dir.join("01Boot.toml");
        write_file(
            &config_path,
            r#"[stage_01.boot_inputs]
overlay_kind = "systemd"
"#,
        );
        write_file(
            &variant_dir.join("ring2-products.toml"),
            r#"schema_version = 6

[ring2_products.rootfs_base]
logical_name = "product.rootfs.base"
description = "Canonical base root filesystem tree"

[ring2_products.live_overlay]
logical_name = "product.payload.live_overlay"
description = "Read-only live overlay payload tree"
overlay_kind = "systemd"

[ring2_products.boot_live]
logical_name = "product.payload.boot.live"
description = "Live boot payload inputs"
extends = "product.rootfs.base"

[ring2_products.live_tools]
logical_name = "product.payload.live_tools"
description = "Live tools payload tree"
extends = "product.payload.boot.live"

[ring2_products.kernel_staging]
logical_name = "product.kernel.staging"
description = "Kernel image and modules staging product"
"#,
        );

        let legacy_inputs = load_legacy_boot_inputs(&config_path)
            .expect("parse legacy config")
            .expect("legacy inputs should exist");
        let overlay = load_ring2_overlay_policy(
            &repo_root,
            &variant_dir,
            &config_path,
            "levitate",
            Some(&legacy_inputs),
        )
        .expect("load ring2 overlay policy");
        assert!(matches!(
            overlay,
            S01OverlayPolicy::Systemd {
                issue_message: None
            }
        ));

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn ring2_overlay_kind_rejects_drift_from_legacy() {
        let repo_root = temp_repo_root("ring2-overlay-drift");
        let variant_dir = repo_root.join("distro-variants/levitate");
        let config_path = variant_dir.join("01Boot.toml");
        write_file(
            &config_path,
            r#"[stage_01.boot_inputs]
overlay_kind = "systemd"
"#,
        );
        write_file(
            &variant_dir.join("ring2-products.toml"),
            r#"schema_version = 6

[ring2_products.rootfs_base]
logical_name = "product.rootfs.base"
description = "Canonical base root filesystem tree"

[ring2_products.live_overlay]
logical_name = "product.payload.live_overlay"
description = "Read-only live overlay payload tree"
overlay_kind = "openrc"

[ring2_products.boot_live]
logical_name = "product.payload.boot.live"
description = "Live boot payload inputs"
extends = "product.rootfs.base"

[ring2_products.live_tools]
logical_name = "product.payload.live_tools"
description = "Live tools payload tree"
extends = "product.payload.boot.live"

[ring2_products.kernel_staging]
logical_name = "product.kernel.staging"
description = "Kernel image and modules staging product"
"#,
        );

        let legacy_inputs = load_legacy_boot_inputs(&config_path)
            .expect("parse legacy config")
            .expect("legacy inputs should exist");
        let err = load_ring2_overlay_policy(
            &repo_root,
            &variant_dir,
            &config_path,
            "levitate",
            Some(&legacy_inputs),
        )
        .expect_err("drifted ring2 overlay kind should fail");
        assert!(
            err.to_string()
                .contains("Ring 2 base-product parity mismatch"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn boot_payload_config_requires_canonical_ring2_owner() {
        let repo_root = temp_repo_root("boot-payload-needs-ring2");
        let variant_dir = repo_root.join("distro-variants/levitate");
        let config_path = variant_dir.join("01Boot.toml");
        write_file(
            &variant_dir.join("identity.toml"),
            r#"schema_version = 6

[identity]
os_name = "LevitateOS"
os_id = "levitateos"
iso_label = "LEVITATE"
os_version = "0.1.0"
default_hostname = "levitate"
"#,
        );
        write_file(
            &variant_dir.join("scenarios.toml"),
            r#"schema_version = 6

[scenarios.live_environment]
required_services = ["sshd"]
"#,
        );
        write_file(
            &config_path,
            r#"[stage_01.boot_inputs]
overlay_kind = "systemd"
"#,
        );

        let err = load_boot_config(&repo_root, &variant_dir, "levitate")
            .expect_err("missing ring2 boot payload owner should fail");
        assert!(
            err.to_string()
                .contains("missing canonical Ring 2 boot payload owner"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn levitate_boot_config_loads_without_01boot_when_ring_files_exist() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");
        let backup_path = variant_dir.join("01Boot.toml.track04-backup");
        let config_path = variant_dir.join("01Boot.toml");

        fs::rename(&config_path, &backup_path).expect("temporarily move 01Boot");
        let loaded = load_boot_config(&repo_root, &variant_dir, "levitate");
        fs::rename(&backup_path, &config_path).expect("restore 01Boot");

        let loaded = loaded.expect("load levitate boot config without 01Boot");
        assert_eq!(loaded.os_name, "LevitateOS");
        assert_eq!(
            loaded.required_services,
            vec!["auditd".to_string(), "sshd".to_string()]
        );
        assert!(matches!(
            loaded.overlay,
            S01OverlayPolicy::Systemd {
                issue_message: None
            }
        ));
        assert!(matches!(
            loaded.rootfs_source_policy,
            Some(S01RootfsSourcePolicy::RecipeRpmDvd { .. })
        ));
        assert!(!loaded.payload_producers.is_empty());
        assert!(loaded
            .payload_producers
            .iter()
            .any(|producer| matches!(producer, RootfsProducer::WriteText { path, .. } if path == Path::new(".live-payload-role"))));
    }

    fn assert_uses_openrc_ring_base_config(
        repo_root: &Path,
        distro_id: &str,
        expected_inittab: InittabVariant,
    ) {
        let variant_dir = repo_root.join(format!("distro-variants/{distro_id}"));
        let loaded = load_boot_config(repo_root, &variant_dir, distro_id)
            .unwrap_or_else(|err| panic!("load {distro_id} boot config: {err:#}"));
        assert!(
            !loaded.payload_producers.is_empty(),
            "expected canonical payload producers for {distro_id}"
        );
        assert!(loaded.payload_producers.iter().any(
            |producer| matches!(producer, RootfsProducer::WriteText { path, .. } if path == Path::new(".live-payload-role"))
        ));
        match loaded.overlay {
            S01OverlayPolicy::OpenRc {
                inittab,
                profile_overlay,
            } => {
                match (inittab, expected_inittab) {
                    (InittabVariant::DesktopWithSerial, InittabVariant::DesktopWithSerial)
                    | (InittabVariant::SerialOnly, InittabVariant::SerialOnly) => {}
                    _ => panic!("unexpected inittab variant for {distro_id}"),
                }
                assert!(
                    profile_overlay.is_some(),
                    "expected profile overlay for {distro_id}"
                );
            }
            other => panic!("unexpected {distro_id} overlay policy: {other:?}"),
        }
    }

    #[test]
    fn acorn_boot_config_uses_openrc_ring_base_config() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_openrc_ring_base_config(&repo_root, "acorn", InittabVariant::DesktopWithSerial);
    }

    #[test]
    fn iuppiter_boot_config_uses_openrc_ring_base_config() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_openrc_ring_base_config(&repo_root, "iuppiter", InittabVariant::SerialOnly);
    }

    #[test]
    fn workspace_variants_load_boot_config_from_repo_wide_ring_owners() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();

        let cases = [
            ("levitate", "LevitateOS", vec!["auditd", "sshd"], "systemd"),
            ("ralph", "RalphOS", vec!["sshd"], "systemd"),
            (
                "acorn",
                "AcornOS",
                vec!["dhcpcd", "networking", "sshd"],
                "openrc",
            ),
            (
                "iuppiter",
                "IuppiterOS",
                vec!["dhcpcd", "networking", "sshd"],
                "openrc",
            ),
        ];

        for (distro_id, expected_os_name, expected_services, expected_overlay_kind) in cases {
            let variant_dir = repo_root.join(format!("distro-variants/{distro_id}"));
            let loaded = load_boot_config(&repo_root, &variant_dir, distro_id)
                .unwrap_or_else(|err| panic!("load {distro_id} boot config: {err:#}"));
            assert_eq!(loaded.os_name, expected_os_name, "unexpected os_name");
            assert_eq!(
                loaded.required_services,
                expected_services
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>(),
                "unexpected required services for {distro_id}"
            );
            assert!(
                !loaded.payload_producers.is_empty(),
                "expected canonical payload producers for {distro_id}"
            );

            match (expected_overlay_kind, loaded.overlay) {
                ("systemd", S01OverlayPolicy::Systemd { .. }) => {}
                (
                    "openrc",
                    S01OverlayPolicy::OpenRc {
                        profile_overlay, ..
                    },
                ) => {
                    assert!(
                        profile_overlay.is_some(),
                        "expected profile overlay for {distro_id}"
                    );
                }
                (expected, other) => panic!(
                    "unexpected overlay policy for {distro_id}: expected {expected}, got {other:?}"
                ),
            }

            match (&loaded.rootfs_source_policy, distro_id) {
                (Some(S01RootfsSourcePolicy::RecipeRpmDvd { .. }), "levitate" | "ralph") => {}
                (Some(S01RootfsSourcePolicy::RecipeCustom { .. }), "acorn" | "iuppiter") => {}
                (other, _) => panic!("unexpected rootfs source policy for {distro_id}: {other:?}"),
            }
        }
    }

    #[test]
    fn workspace_variants_load_live_tools_config_from_canonical_owners() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();

        let cases = [
            ("levitate", "LevitateOS", InstallExperience::Ux, 5usize),
            ("ralph", "RalphOS", InstallExperience::AutomatedSsh, 4usize),
            ("acorn", "AcornOS", InstallExperience::Ux, 6usize),
            (
                "iuppiter",
                "IuppiterOS",
                InstallExperience::AutomatedSsh,
                7usize,
            ),
        ];

        for (distro_id, expected_os_name, expected_install_experience, minimum_actions) in cases {
            let variant_dir = repo_root.join(format!("distro-variants/{distro_id}"));
            let loaded = load_live_tools_config(&variant_dir)
                .unwrap_or_else(|err| panic!("load {distro_id} live-tools config: {err:#}"));
            assert_eq!(loaded.os_name, expected_os_name, "unexpected os_name");
            assert_eq!(
                loaded.install_experience, expected_install_experience,
                "unexpected install experience for {distro_id}"
            );
            assert!(
                loaded.runtime_actions.len() >= minimum_actions,
                "expected canonical runtime actions for {distro_id}"
            );
        }
    }

    #[test]
    fn installed_boot_payload_config_uses_ring2_profile() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");

        let loaded = load_installed_boot_payload_config(&repo_root, &variant_dir, "levitate")
            .expect("load installed boot payload config");

        assert!(loaded.payload_producers.iter().any(
            |producer| matches!(producer, RootfsProducer::CopySymlink { source, destination } if source == Path::new("bin") && destination == Path::new("bin"))
        ));
    }
}
