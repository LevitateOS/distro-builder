//! OpenRC operation handlers: service enabling, init script copying, conf.d writing.
//!
//! These operations handle OpenRC-specific service management for
//! Alpine-based distributions (AcornOS, IuppiterOS).

use anyhow::{bail, Result};
use std::fs;
use std::path::Path;

use super::binaries::make_executable;

/// Enable an OpenRC service in a runlevel.
///
/// Creates symlink: /etc/runlevels/<runlevel>/<service> -> /etc/init.d/<service>
pub fn enable_service(staging: &Path, service: &str, runlevel: &str) -> Result<()> {
    let runlevel_dir = staging.join("etc/runlevels").join(runlevel);
    fs::create_dir_all(&runlevel_dir)?;

    let link = runlevel_dir.join(service);
    let target = format!("/etc/init.d/{}", service);

    if !link.exists() && !link.is_symlink() {
        std::os::unix::fs::symlink(&target, &link)?;
    }

    Ok(())
}

/// Copy an OpenRC init script from source to staging.
///
/// Fails if the script doesn't exist in source - all listed scripts are required.
pub fn copy_init_script(source: &Path, staging: &Path, script: &str) -> Result<()> {
    let src = source.join("etc/init.d").join(script);
    let dst = staging.join("etc/init.d").join(script);

    if !src.exists() {
        bail!(
            "OpenRC init script not found: {}\n\
             This script is required. Check that the corresponding package is installed in alpine.rhai.",
            src.display()
        );
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::copy(&src, &dst)?;
    make_executable(&dst)?;

    Ok(())
}

/// Write an OpenRC conf.d configuration file.
pub fn write_conf(staging: &Path, service: &str, content: &str) -> Result<()> {
    let conf_path = staging.join("etc/conf.d").join(service);
    if let Some(parent) = conf_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&conf_path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_enable_service() {
        let temp = TempDir::new().unwrap();
        let staging = temp.path();

        enable_service(staging, "sshd", "default").unwrap();

        let link = staging.join("etc/runlevels/default/sshd");
        assert!(link.is_symlink());
        assert_eq!(
            fs::read_link(&link).unwrap().to_str().unwrap(),
            "/etc/init.d/sshd"
        );
    }

    #[test]
    fn test_enable_service_idempotent() {
        let temp = TempDir::new().unwrap();
        let staging = temp.path();

        enable_service(staging, "sshd", "default").unwrap();
        enable_service(staging, "sshd", "default").unwrap();

        assert!(staging.join("etc/runlevels/default/sshd").is_symlink());
    }

    #[test]
    fn test_copy_init_script_missing() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        let staging = temp.path().join("staging");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&staging).unwrap();

        let result = copy_init_script(&source, &staging, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_write_conf() {
        let temp = TempDir::new().unwrap();
        let staging = temp.path();

        write_conf(staging, "sshd", "SSHD_OPTS=\"-p 22\"").unwrap();

        let content = fs::read_to_string(staging.join("etc/conf.d/sshd")).unwrap();
        assert_eq!(content, "SSHD_OPTS=\"-p 22\"");
    }
}
