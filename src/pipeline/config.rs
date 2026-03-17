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
#[cfg(test)]
use crate::pipeline::source::load_rootfs_source_policy;
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

#[cfg(test)]
pub(crate) fn load_boot_payload_config(
    repo_root: &Path,
    variant_dir: &Path,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    let mut loaded = load_boot_payload_config_with_rootfs_source_policy(repo_root, contract, None)?;
    let rootfs_source_policy = load_rootfs_source_policy(repo_root, variant_dir)?;
    loaded.rootfs_source_policy = rootfs_source_policy;
    Ok(loaded)
}

pub(crate) fn load_boot_payload_config_from_contract(
    repo_root: &Path,
    contract: &ConformanceContract,
) -> Result<BootLoadedConfig> {
    let rootfs_source_policy = rootfs_source_policy_from_contract(repo_root, contract)?;
    load_boot_payload_config_with_rootfs_source_policy(repo_root, contract, rootfs_source_policy)
}

#[cfg(test)]
pub(crate) fn load_boot_config(
    repo_root: &Path,
    variant_dir: &Path,
    contract: &ConformanceContract,
    distro_id: &str,
) -> Result<BootLoadedConfig> {
    let loaded = load_boot_payload_config(repo_root, variant_dir, contract)?;
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

#[cfg(test)]
pub(crate) fn load_installed_boot_payload_config(
    repo_root: &Path,
    variant_dir: &Path,
    contract: &ConformanceContract,
    _distro_id: &str,
) -> Result<BootLoadedConfig> {
    let mut loaded = load_boot_payload_config(repo_root, variant_dir, contract)?;
    loaded.payload_producers =
        load_boot_payload_producers(contract, BootPayloadProduct::Installed)?;
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
        RuntimeActionContract::IuppiterDarPayload { target } => {
            LiveToolsRuntimeAction::IuppiterDarPayload {
                target: target
                    .as_deref()
                    .map(|value| normalize_string(value, "target"))
                    .transpose()?,
            }
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
            let profile_overlay = overlay
                .profile_overlay
                .as_ref()
                .map(|path| resolve_repo_path(repo_root, path));
            Ok(BootOverlayPolicy::OpenRc {
                inittab,
                profile_overlay,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::source::RootfsSourcePolicy;
    use distro_contract::load_variant_contract_for_distro_from;
    use std::path::Component;
    use std::path::PathBuf;

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

    fn workspace_contract(distro_id: &str) -> ConformanceContract {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        load_variant_contract_for_distro_from(&repo_root, distro_id)
            .unwrap_or_else(|err| panic!("load workspace contract for {distro_id}: {err:#}"))
    }

    fn assert_uses_fedora_dvd_source_recipes(repo_root: &Path, distro_id: &str) {
        let variant_dir = repo_root.join(format!("distro-variants/{distro_id}"));
        let contract = workspace_contract(distro_id);
        let loaded = load_boot_config(repo_root, &variant_dir, &contract, distro_id)
            .unwrap_or_else(|err| panic!("load {distro_id} live-boot config: {err:#}"));
        match loaded.rootfs_source_policy {
            Some(RootfsSourcePolicy::RecipeRpmDvd {
                recipe_script,
                preseed_recipe_script,
            }) => {
                assert!(
                    recipe_script.ends_with("distro-builder/recipes/fedora-dvd-source-rootfs.rhai"),
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
    fn levitate_boot_config_uses_fedora_dvd_source_recipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_fedora_dvd_source_recipes(&repo_root, "levitate");
    }

    #[test]
    fn ralph_boot_config_uses_fedora_dvd_source_recipes() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        assert_uses_fedora_dvd_source_recipes(&repo_root, "ralph");
    }

    #[test]
    fn ring2_overlay_policy_loads_from_canonical_owner() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let contract = workspace_contract("levitate");

        let overlay =
            load_ring2_overlay_policy(&repo_root, &contract).expect("load ring2 overlay policy");
        assert!(matches!(
            overlay,
            BootOverlayPolicy::Systemd {
                issue_message: None
            }
        ));
    }

    #[test]
    fn ring2_overlay_policy_requires_openrc_inittab() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let mut contract = workspace_contract("acorn");
        contract.product_config.live_overlay.openrc_inittab = None;

        let err = load_ring2_overlay_policy(&repo_root, &contract)
            .expect_err("missing openrc inittab should fail");
        assert!(
            err.to_string()
                .contains("openrc overlay requires openrc_inittab"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn boot_payload_config_requires_contract_boot_payload_producers() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");
        let mut contract = workspace_contract("levitate");
        contract.product_config.boot_live.producers.clear();

        let err = load_boot_config(&repo_root, &variant_dir, &contract, "levitate")
            .expect_err("missing boot payload producers should fail");
        assert!(
            err.to_string()
                .contains("missing canonical boot payload producers"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn boot_config_requires_contract_live_environment_to_include_sshd() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");
        let mut contract = workspace_contract("levitate");
        contract.scenarios.live_environment.required_services = vec!["auditd".to_string()];

        let err = load_boot_config(&repo_root, &variant_dir, &contract, "levitate")
            .expect_err("missing sshd in contract live environment should fail");
        assert!(
            err.to_string()
                .contains("required_services must include 'sshd'"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn levitate_boot_config_loads_from_ring_files() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repo root")
            .to_path_buf();
        let variant_dir = repo_root.join("distro-variants/levitate");
        let contract = workspace_contract("levitate");

        let loaded = load_boot_config(&repo_root, &variant_dir, &contract, "levitate")
            .expect("load levitate boot config from ring files");
        assert_eq!(loaded.os_name, "LevitateOS");
        assert_eq!(
            loaded.required_services,
            vec!["auditd".to_string(), "sshd".to_string()]
        );
        assert!(matches!(
            loaded.overlay,
            BootOverlayPolicy::Systemd {
                issue_message: None
            }
        ));
        assert!(matches!(
            loaded.rootfs_source_policy,
            Some(RootfsSourcePolicy::RecipeRpmDvd { .. })
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
        let contract = workspace_contract(distro_id);
        let loaded = load_boot_config(repo_root, &variant_dir, &contract, distro_id)
            .unwrap_or_else(|err| panic!("load {distro_id} boot config: {err:#}"));
        assert!(
            !loaded.payload_producers.is_empty(),
            "expected canonical payload producers for {distro_id}"
        );
        assert!(loaded.payload_producers.iter().any(
            |producer| matches!(producer, RootfsProducer::WriteText { path, .. } if path == Path::new(".live-payload-role"))
        ));
        match loaded.overlay {
            BootOverlayPolicy::OpenRc {
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
            let contract = workspace_contract(distro_id);
            let loaded = load_boot_config(&repo_root, &variant_dir, &contract, distro_id)
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
                ("systemd", BootOverlayPolicy::Systemd { .. }) => {}
                (
                    "openrc",
                    BootOverlayPolicy::OpenRc {
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
                (Some(RootfsSourcePolicy::RecipeRpmDvd { .. }), "levitate" | "ralph") => {}
                (Some(RootfsSourcePolicy::RecipeCustom { .. }), "acorn" | "iuppiter") => {}
                (other, _) => panic!("unexpected rootfs source policy for {distro_id}: {other:?}"),
            }
        }
    }

    #[test]
    fn workspace_variants_load_live_tools_config_from_canonical_owners() {
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
            let contract = workspace_contract(distro_id);
            let loaded = load_live_tools_config_from_contract(&contract)
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
        let contract = workspace_contract("levitate");

        let loaded =
            load_installed_boot_payload_config(&repo_root, &variant_dir, &contract, "levitate")
                .expect("load installed boot payload config");

        assert!(loaded.payload_producers.iter().any(
            |producer| matches!(producer, RootfsProducer::CopySymlink { source, destination } if source == Path::new("bin") && destination == Path::new("bin"))
        ));
    }
}
