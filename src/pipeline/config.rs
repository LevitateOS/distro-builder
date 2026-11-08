use anyhow::{bail, Result};
use distro_contract::{
    ConformanceContract, InstallUxContract, PayloadProducerContract, Profile,
    RuntimeActionContract,
};
use std::path::Path;

use crate::pipeline::plan::RootfsProducer;

use crate::pipeline::source::{rootfs_source_policy_from_contract, RootfsSourcePolicy};

#[derive(Debug, Clone)]
pub(crate) struct BootLoadedConfig {
    pub(crate) os_name: String,
    pub(crate) required_services: Vec<String>,
    pub(crate) rootfs_source_policy: Option<RootfsSourcePolicy>,
    pub(crate) profile: Profile,
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
    // The contract guarantees distinct service names; the overlay takes them sorted.
    let mut required_services = contract.scenarios.live_environment.required_services.clone();
    required_services.sort();
    let payload_producers = load_boot_payload_producers(contract, BootPayloadProduct::Live)?;

    Ok(BootLoadedConfig {
        os_name,
        required_services,
        rootfs_source_policy,
        profile: contract.profile,
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

/// The contract validates the producer paths as relative paths below the rootfs.
fn rootfs_producer_from_contract(producer: &PayloadProducerContract) -> Result<RootfsProducer> {
    use std::path::PathBuf;
    Ok(match producer {
        PayloadProducerContract::CopyTree {
            source,
            destination,
        } => RootfsProducer::CopyTree {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
        },
        PayloadProducerContract::CopySymlink {
            source,
            destination,
        } => RootfsProducer::CopySymlink {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
        },
        PayloadProducerContract::CopyFile {
            source,
            destination,
            optional,
        } => RootfsProducer::CopyFile {
            source: PathBuf::from(source),
            destination: PathBuf::from(destination),
            optional: *optional,
        },
        PayloadProducerContract::WriteText {
            path,
            content,
            mode,
        } => RootfsProducer::WriteText {
            path: PathBuf::from(path),
            content: content.clone(),
            mode: *mode,
        },
    })
}
