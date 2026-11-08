use anyhow::Result;
use distro_contract::{ConformanceContract, InstallUxContract, Profile};
use std::path::{Path, PathBuf};

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
    pub(crate) install_ux: InstallUxContract,
}

/// Which system a boot payload is.
#[derive(Debug, Clone, Copy)]
enum BootPayload {
    Live,
    Installed,
}

/// Both payloads are the whole Recipe-built system tree. The live one is
/// marked as the live init's rootfs layer (recinit's payload-role protocol).
fn boot_payload_producers(payload: BootPayload) -> Vec<RootfsProducer> {
    let system = RootfsProducer::CopyTree {
        source: PathBuf::from("."),
        destination: PathBuf::from("."),
    };
    match payload {
        BootPayload::Live => vec![
            system,
            RootfsProducer::WriteText {
                path: PathBuf::from(recinit::live::PAYLOAD_ROLE_FILE),
                content: format!("{}\n", recinit::live::ROLE_ROOTFS),
            },
        ],
        BootPayload::Installed => vec![system],
    }
}

fn load_boot_payload_config(
    repo_root: &Path,
    contract: &ConformanceContract,
    payload: BootPayload,
) -> Result<BootLoadedConfig> {
    Ok(BootLoadedConfig {
        os_name: contract.identity.os_name.clone(),
        required_services: contract.live_service_units(),
        rootfs_source_policy: rootfs_source_policy_from_contract(repo_root, contract)?,
        profile: contract.profile,
        payload_producers: boot_payload_producers(payload),
    })
}

pub(crate) fn load_boot_config_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    load_boot_payload_config(repo_root, contract, BootPayload::Live)
}

pub(crate) fn load_installed_boot_payload_config_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    load_boot_payload_config(repo_root, contract, BootPayload::Installed)
}

pub(crate) fn load_live_tools_config_from_contract(
    contract: &ConformanceContract,
) -> LiveToolsLoadedConfig {
    LiveToolsLoadedConfig {
        os_name: contract.identity.os_name.clone(),
        install_ux: contract.install_ux.clone(),
    }
}
