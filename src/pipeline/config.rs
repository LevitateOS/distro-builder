use anyhow::{bail, Result};
use distro_contract::{
    ConformanceContract, InstallUxContract, OpenRcInittab, OverlayContract, OverlayKind,
    PayloadProducerContract, RuntimeActionContract,
};
use std::path::Path;

use crate::pipeline::overlay::BootOverlayPolicy;
use crate::pipeline::plan::RootfsProducer;

use crate::artifact::live_overlay::InittabVariant;
use crate::pipeline::source::{rootfs_source_policy_from_contract, RootfsSourcePolicy};

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
    pub(crate) runtime_actions: Vec<RuntimeActionContract>,
    pub(crate) install_ux: InstallUxContract,
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
    let overlay = overlay_policy_from_contract(&contract.product_config.live_overlay)?;
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
    load_boot_payload_config_with_rootfs_source_policy(contract, rootfs_source_policy)
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
    Ok(LiveToolsLoadedConfig {
        os_name,
        runtime_actions: contract.product_config.live_tools_actions.clone(),
        install_ux: contract.product_config.install_ux.clone(),
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

fn overlay_policy_from_contract(overlay: &OverlayContract) -> Result<BootOverlayPolicy> {
    match overlay.kind {
        OverlayKind::Systemd => Ok(BootOverlayPolicy::Systemd),
        OverlayKind::OpenRc => {
            let inittab = match overlay.openrc_inittab {
                Some(OpenRcInittab::DesktopWithSerial) => InittabVariant::DesktopWithSerial,
                Some(OpenRcInittab::SerialOnly) => InittabVariant::SerialOnly,
                None => bail!(
                    "invalid canonical live-overlay config: openrc overlay requires openrc_inittab"
                ),
            };
            Ok(BootOverlayPolicy::OpenRc { inittab })
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
