use anyhow::{bail, Result};
use distro_contract::{
    ConformanceContract, InstallDocsFrontend as ContractInstallDocsFrontend,
    InstallExperience as ContractInstallExperience, OpenRcInittab, OverlayContract, OverlayKind,
    PayloadProducerContract, RuntimeActionContract,
};
use std::path::Path;

use crate::pipeline::live_tools::{InstallDocsFrontend, InstallExperience, LiveToolsRuntimeAction};
use crate::pipeline::overlay::BootOverlayPolicy;
use crate::pipeline::paths::resolve_repo_path;
use crate::pipeline::plan::RootfsProducer;

use crate::pipeline::source::{rootfs_source_policy_from_contract, RootfsSourcePolicy};
use crate::InittabVariant;

#[derive(Debug, Clone)]
pub(crate) struct BootLoadedConfig {
    pub(crate) os_name: String,
    pub(crate) required_services: Vec<String>,
    pub(crate) rootfs_source_policy: Option<RootfsSourcePolicy>,
    pub(crate) overlay: BootOverlayPolicy,
    pub(crate) payload_producers: Vec<RootfsProducer>,
}

#[derive(Debug, Clone)]
pub(crate) struct LiveToolsLoadedConfig {
    pub(crate) os_name: String,
    pub(crate) install_experience: InstallExperience,
    pub(crate) runtime_actions: Vec<LiveToolsRuntimeAction>,
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

fn load_boot_payload_config_with_rootfs_source_policy(
    repo_root: &Path,
    contract: &ConformanceContract,
    rootfs_source_policy: Option<RootfsSourcePolicy>,
) -> Result<BootLoadedConfig> {
    let os_name = contract.identity.os_name.clone();
    let required_services = normalize_services(
        contract
            .scenarios
            .live_environment
            .required_services
            .clone(),
    );
    let overlay = load_ring2_overlay_policy(repo_root, contract)?;
    let payload_producers = load_boot_payload_producers(contract, BootPayloadProduct::Live)?;

    Ok(BootLoadedConfig {
        os_name,
        required_services,
        rootfs_source_policy,
        overlay,
        payload_producers,
    })
}

pub(crate) fn load_boot_payload_config_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    let rootfs_source_policy = rootfs_source_policy_from_contract(repo_root, contract)?;
    load_boot_payload_config_with_rootfs_source_policy(repo_root, contract, rootfs_source_policy)
}

pub(crate) fn load_boot_config_from_contract(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    let loaded = load_boot_payload_config_from_contract(repo_root, contract)?;
    if !loaded.required_services.iter().any(|svc| svc == "sshd") {
        bail!(
            "invalid live boot config for '{}': required_services must include 'sshd' (OpenSSH is first-class in live boot)",
            distro_id
        );
    }
    Ok(loaded)
}

pub(crate) fn load_installed_boot_payload_config_from_contract(
    repo_root: &Path,
    _distro_id: &str,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    let mut loaded = load_boot_payload_config_from_contract(repo_root, contract)?;
    loaded.payload_producers =
        load_boot_payload_producers(contract, BootPayloadProduct::Installed)?;
    Ok(loaded)
}

pub(crate) fn load_live_tools_config_from_contract(
    contract: &ConformanceContract,
) -> Result<LiveToolsLoadedConfig> {
    let os_name = contract.identity.os_name.clone();
    let install_experience = install_experience_from_contract(contract);
    let runtime_actions = load_live_tools_runtime_actions(contract, install_experience)?;

    Ok(LiveToolsLoadedConfig {
        os_name,
        install_experience,
        runtime_actions,
    })
}

fn load_boot_payload_producers(
    contract: &ConformanceContract,
    product: BootPayloadProduct,
) -> Result<Vec<RootfsProducer>> {
    let producers = match product {
        BootPayloadProduct::Live => &contract.product_config.boot_live.producers,
        BootPayloadProduct::Installed => contract
            .product_config
            .boot_installed
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "missing canonical boot payload config for '{}'",
                    product.logical_name()
                )
            })?
            .producers
            .as_slice(),
    };

    if producers.is_empty() {
        bail!(
            "missing canonical boot payload producers for '{}': populate contract.product_config",
            product.logical_name()
        );
    }

    producers
        .iter()
        .map(rootfs_producer_from_contract)
        .collect()
}

fn load_live_tools_runtime_actions(
    contract: &ConformanceContract,
    install_experience: InstallExperience,
) -> Result<Vec<LiveToolsRuntimeAction>> {
    let live_tools = &contract.product_config.live_tools;
    let mut actions = live_tools.common_actions.clone();
    match install_experience {
        InstallExperience::Ux => actions.extend(live_tools.ux_actions.clone()),
        InstallExperience::AutomatedSsh => {
            actions.extend(live_tools.automated_ssh_actions.clone());
        }
    }

    if actions.is_empty() {
        bail!(
            "missing canonical live-tools runtime actions for '{}': populate contract.product_config.live_tools for install experience '{}'",
            contract.products.live_tools.logical_name,
            install_experience.as_str()
        );
    }

    actions
        .iter()
        .map(live_tools_runtime_action_from_contract)
        .collect()
}

fn live_tools_runtime_action_from_contract(
    action: &RuntimeActionContract,
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
        RuntimeActionContract::ToolPayloadWorkspaceBinary {
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
        RuntimeActionContract::RootfsWorkspaceBinary {
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
        RuntimeActionContract::ApkPackages { packages } => {
            let packages = packages
                .iter()
                .map(|package| normalize_string(package, "packages"))
                .collect::<Result<Vec<_>>>()?;
            if packages.is_empty() {
                bail!("Ring 2 runtime action 'apk_packages' must declare at least one package");
            }
            LiveToolsRuntimeAction::ApkPackages { packages }
        }
        RuntimeActionContract::InstallModePayload {
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
                ux_docs_frontend: match ux_docs_frontend {
                    ContractInstallDocsFrontend::PlainText => InstallDocsFrontend::PlainText,
                    ContractInstallDocsFrontend::BunBundle => InstallDocsFrontend::BunBundle,
                },
            }
        }
    })
}

fn rootfs_producer_from_contract(producer: &PayloadProducerContract) -> Result<RootfsProducer> {
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
        PayloadProducerContract::CopyTree {
            source,
            destination,
        } => RootfsProducer::CopyTree {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
        },
        PayloadProducerContract::CopySymlink {
            source,
            destination,
        } => RootfsProducer::CopySymlink {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
        },
        PayloadProducerContract::CopyFile {
            source,
            destination,
            optional,
        } => RootfsProducer::CopyFile {
            source: normalized_relative_path(source, "source")?,
            destination: normalized_relative_path(destination, "destination")?,
            optional: *optional,
        },
        PayloadProducerContract::WriteText {
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
fn install_experience_from_contract(contract: &ConformanceContract) -> InstallExperience {
    match contract.scenarios.live_tools.install_experience {
        ContractInstallExperience::Ux => InstallExperience::Ux,
        ContractInstallExperience::AutomatedSsh => InstallExperience::AutomatedSsh,
    }
}

fn load_ring2_overlay_policy(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<BootOverlayPolicy> {
    overlay_policy_from_contract(repo_root, &contract.product_config.live_overlay)
}

fn overlay_policy_from_contract(
    repo_root: &Path,
    overlay: &OverlayContract,
) -> Result<BootOverlayPolicy> {
    match overlay.kind {
        OverlayKind::Systemd => Ok(BootOverlayPolicy::Systemd {
            issue_message: overlay.issue_message.clone(),
        }),
        OverlayKind::OpenRc => {
            let inittab = match overlay.openrc_inittab {
                Some(OpenRcInittab::DesktopWithSerial) => InittabVariant::DesktopWithSerial,
                Some(OpenRcInittab::SerialOnly) => InittabVariant::SerialOnly,
                None => bail!(
                    "invalid canonical live-overlay config: openrc overlay requires openrc_inittab"
                ),
            };
            let seed_overlay = overlay
                .seed_overlay
                .as_ref()
                .map(|path| resolve_repo_path(repo_root, path));
            Ok(BootOverlayPolicy::OpenRc {
                inittab,
                seed_overlay,
            })
        }
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
