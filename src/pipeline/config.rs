use anyhow::{bail, Context, Result};
use distro_contract::ConformanceContract;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::pipeline::live_tools::{InstallDocsFrontend, InstallExperience, LiveToolsRuntimeAction};
use crate::pipeline::overlay::S01OverlayPolicy;
use crate::pipeline::paths::resolve_repo_path;
use crate::pipeline::plan::RootfsProducer;
#[cfg(test)]
use crate::pipeline::source::load_rootfs_source_policy;
use crate::pipeline::source::{rootfs_source_policy_from_contract, S01RootfsSourcePolicy};
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
    #[allow(dead_code)]
    live_boot: Option<LiveBootScenarioToml>,
    live_environment: Option<LiveEnvironmentScenarioToml>,
    #[allow(dead_code)]
    live_tools: Option<LiveToolsScenarioToml>,
    #[allow(dead_code)]
    install: Option<InstallScenarioToml>,
    #[allow(dead_code)]
    installed_boot: Option<BootScenarioToml>,
    #[allow(dead_code)]
    automated_login: Option<AutomatedLoginScenarioToml>,
    #[allow(dead_code)]
    installed_tools: Option<ToolsScenarioToml>,
    #[allow(dead_code)]
    runtime_policy: Option<RuntimePolicyScenarioToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveBootScenarioToml {
    #[allow(dead_code)]
    #[serde(default)]
    success_patterns: Vec<String>,
    #[allow(dead_code)]
    #[serde(default)]
    fatal_patterns: Vec<String>,
    #[allow(dead_code)]
    required_kernel_cmdline: Vec<String>,
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    #[serde(default)]
    required_tools: Vec<String>,
    install_experience: InstallExperience,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallScenarioToml {
    #[allow(dead_code)]
    required_tools: Vec<String>,
    #[allow(dead_code)]
    required_services: Vec<String>,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BootScenarioToml {
    #[allow(dead_code)]
    success_patterns: Vec<String>,
    #[allow(dead_code)]
    fatal_patterns: Vec<String>,
    #[allow(dead_code)]
    required_kernel_cmdline: Vec<String>,
    #[allow(dead_code)]
    required_live_services: Vec<String>,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AutomatedLoginScenarioToml {
    #[allow(dead_code)]
    auth_mode: AuthModeToml,
    #[allow(dead_code)]
    default_username: Option<String>,
    #[allow(dead_code)]
    default_password: Option<String>,
    #[allow(dead_code)]
    login_prompt_pattern: String,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolsScenarioToml {
    #[allow(dead_code)]
    required_tools: Vec<String>,
    #[allow(dead_code)]
    evidence: Option<ScenarioEvidenceToml>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimePolicyScenarioToml {
    #[allow(dead_code)]
    rootfs_mutability: RootfsMutabilityToml,
    #[allow(dead_code)]
    mutable_required_rw_paths: Vec<String>,
    #[allow(dead_code)]
    immutable_required_ro_paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AuthModeToml {
    DefaultPasswordLogin,
    ProvisionedCredentials,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RootfsMutabilityToml {
    Mutable,
    Immutable,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioEvidenceToml {
    #[allow(dead_code)]
    script_path: String,
    #[allow(dead_code)]
    pass_marker: String,
}

fn load_boot_payload_config_with_rootfs_source_policy(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    rootfs_source_policy: Option<S01RootfsSourcePolicy>,
) -> Result<S01LoadedConfig> {
    let os_name = load_identity_os_name(variant_dir)?;
    let required_services = load_required_services(variant_dir)?;
    let overlay = load_ring2_overlay_policy(repo_root, variant_dir, distro_id)?;
    let payload_producers =
        load_boot_payload_producers(variant_dir, BootPayloadProduct::Live, &overlay)?;

    Ok(S01LoadedConfig {
        os_name,
        required_services,
        rootfs_source_policy,
        overlay,
        payload_producers,
    })
}

#[cfg(test)]
pub(crate) fn load_boot_payload_config(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01LoadedConfig> {
    let mut loaded = load_boot_payload_config_with_rootfs_source_policy(
        repo_root,
        variant_dir,
        distro_id,
        None,
    )?;
    let rootfs_source_policy = load_rootfs_source_policy(repo_root, variant_dir)?;
    loaded.rootfs_source_policy = rootfs_source_policy;
    Ok(loaded)
}

pub(crate) fn load_boot_payload_config_from_contract(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
) -> Result<S01LoadedConfig> {
    let rootfs_source_policy = rootfs_source_policy_from_contract(repo_root, contract)?;
    load_boot_payload_config_with_rootfs_source_policy(
        repo_root,
        variant_dir,
        distro_id,
        rootfs_source_policy,
    )
}

#[cfg(test)]
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

pub(crate) fn load_boot_config_from_contract(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
) -> Result<S01LoadedConfig> {
    let loaded =
        load_boot_payload_config_from_contract(repo_root, variant_dir, distro_id, contract)?;
    if !loaded.required_services.iter().any(|svc| svc == "sshd") {
        bail!(
            "invalid live boot config for '{}': required_services must include 'sshd' (OpenSSH is first-class in live boot)",
            distro_id
        );
    }
    Ok(loaded)
}

#[cfg(test)]
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

pub(crate) fn load_installed_boot_payload_config_from_contract(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
) -> Result<S01LoadedConfig> {
    let mut loaded =
        load_boot_payload_config_from_contract(repo_root, variant_dir, distro_id, contract)?;
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
fn load_identity_os_name(variant_dir: &Path) -> Result<String> {
    load_live_tools_os_name(variant_dir)
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

fn load_required_services(variant_dir: &Path) -> Result<Vec<String>> {
    let scenarios_path = variant_dir.join("scenarios.toml");
    if !scenarios_path.is_file() {
        bail!(
            "missing canonical live-environment scenario owner for '{}': expected '{}'",
            variant_dir.display(),
            scenarios_path.display()
        );
    }

    let config_bytes = fs::read_to_string(&scenarios_path)
        .with_context(|| format!("reading scenarios config '{}'", scenarios_path.display()))?;
    let parsed: ScenariosToml = toml::from_str(&config_bytes)
        .with_context(|| format!("parsing scenarios config '{}'", scenarios_path.display()))?;
    let ring_required_services = parsed
        .scenarios
        .live_environment
        .map(|env| normalize_services(env.required_services))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "missing canonical live-environment scenario owner '[scenarios.live_environment]' in '{}'",
                scenarios_path.display()
            )
        })?;

    Ok(ring_required_services)
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
    distro_id: &str,
) -> Result<S01OverlayPolicy> {
    let ring2_config_path = variant_dir.join("ring2-products.toml");
    if !ring2_config_path.is_file() {
        bail!(
            "missing canonical Ring 2 base-product owner for '{}': expected '{}'",
            distro_id,
            ring2_config_path.display()
        );
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

    overlay_policy_from_ring(
        repo_root,
        &ring2_config_path,
        distro_id,
        parsed.ring2_products.live_overlay,
    )
}

fn parse_openrc_inittab(
    value: Option<&str>,
    config_path: &Path,
    distro_id: &str,
) -> Result<InittabVariant> {
    let raw = value.ok_or_else(|| {
        anyhow::anyhow!(
            "invalid live-boot overlay config '{}': openrc_inittab is required for distro '{}'",
            config_path.display(),
            distro_id
        )
    })?;

    match raw.trim().to_ascii_lowercase().as_str() {
        "desktop_with_serial" => Ok(InittabVariant::DesktopWithSerial),
        "serial_only" => Ok(InittabVariant::SerialOnly),
        other => bail!(
            "invalid live-boot overlay config '{}': unsupported openrc_inittab '{}' for distro '{}'",
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
            .unwrap_or_else(|err| panic!("load {distro_id} live-boot config: {err:#}"));
        match loaded.rootfs_source_policy {
            Some(S01RootfsSourcePolicy::RecipeRpmDvd {
                recipe_script,
                preseed_recipe_script,
            }) => {
                assert!(
                    recipe_script.ends_with("distro-builder/recipes/fedora-stage01-rootfs.rhai"),
                    "unexpected live-boot recipe: {}",
                    recipe_script.display()
                );
                assert!(
                    preseed_recipe_script
                        .ends_with("distro-builder/recipes/fedora-preseed-iso.rhai"),
                    "unexpected live-boot preseed recipe: {}",
                    preseed_recipe_script.display()
                );
            }
            other => panic!("unexpected {distro_id} rootfs source policy: {other:?}"),
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
    fn ring2_overlay_policy_loads_from_canonical_owner() {
        let repo_root = temp_repo_root("ring2-overlay-canonical");
        let variant_dir = repo_root.join("distro-variants/levitate");
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

        let overlay = load_ring2_overlay_policy(&repo_root, &variant_dir, "levitate")
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
    fn ring2_overlay_policy_requires_canonical_owner() {
        let repo_root = temp_repo_root("ring2-overlay-missing");
        let variant_dir = repo_root.join("distro-variants/levitate");
        let err = load_ring2_overlay_policy(&repo_root, &variant_dir, "levitate")
            .expect_err("missing ring2 overlay policy should fail");
        assert!(
            err.to_string()
                .contains("missing canonical Ring 2 base-product owner"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn boot_payload_config_requires_canonical_ring2_owner() {
        let repo_root = temp_repo_root("boot-payload-needs-ring2");
        let variant_dir = repo_root.join("distro-variants/levitate");
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

        let err = load_boot_config(&repo_root, &variant_dir, "levitate")
            .expect_err("missing ring2 boot payload owner should fail");
        assert!(
            err.to_string()
                .contains("missing canonical Ring 2 base-product owner"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn boot_payload_config_requires_canonical_scenarios_owner() {
        let repo_root = temp_repo_root("boot-payload-needs-scenarios");
        let variant_dir = repo_root.join("distro-variants/levitate");
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

        let err = load_boot_config(&repo_root, &variant_dir, "levitate")
            .expect_err("missing scenarios owner should fail");
        assert!(
            err.to_string()
                .contains("missing canonical live-environment scenario owner"),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn boot_payload_config_requires_live_environment_owner_section() {
        let repo_root = temp_repo_root("boot-payload-live-environment-missing");
        let variant_dir = repo_root.join("distro-variants/levitate");
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

[scenarios.live_boot]
required_kernel_cmdline = ["audit=1"]
required_live_services = ["sshd"]
"#,
        );

        let err = load_boot_config(&repo_root, &variant_dir, "levitate")
            .expect_err("missing live_environment section should fail");
        assert!(
            err.to_string().contains(
                "missing canonical live-environment scenario owner '[scenarios.live_environment]'"
            ),
            "unexpected error: {err:#}"
        );

        fs::remove_dir_all(repo_root).expect("cleanup temp root");
    }

    #[test]
    fn levitate_boot_config_loads_from_ring_files() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");

        let loaded = load_boot_config(&repo_root, &variant_dir, "levitate")
            .expect("load levitate boot config from ring files");
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
