use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::pipeline::overlay::S01OverlayPolicy;
use crate::pipeline::paths::resolve_repo_path;
use crate::pipeline::source::{
    parse_rootfs_source_policy, S01RootfsSourcePolicy, S01RootfsSourceToml,
};
use crate::InittabVariant;

#[derive(Debug, Clone)]
pub(crate) struct S01LoadedConfig {
    pub(crate) os_name: String,
    pub(crate) required_services: Vec<String>,
    pub(crate) rootfs_source_policy: Option<S01RootfsSourcePolicy>,
    pub(crate) overlay: S01OverlayPolicy,
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

pub(crate) fn load_boot_config(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01LoadedConfig> {
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

    let rootfs_source_policy =
        parse_rootfs_source_policy(repo_root, &config_path, boot_inputs.rootfs_source.clone())?;

    Ok(S01LoadedConfig {
        os_name: boot_inputs.os_name,
        required_services,
        rootfs_source_policy,
        overlay,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
