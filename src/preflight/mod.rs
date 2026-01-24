//! Preflight checks for build validation.
//!
//! Validates that the host system has required tools before building.
//! This prevents cryptic errors during the build process.
//!
//! # Example
//!
//! ```rust
//! use distro_builder::preflight::{command_exists, check_required_tools};
//!
//! // Check a single command
//! if !command_exists("mksquashfs") {
//!     println!("squashfs-tools not installed");
//! }
//!
//! // Check multiple tools
//! let tools = &[("mksquashfs", "squashfs-tools"), ("xorriso", "xorriso")];
//! if let Err(e) = check_required_tools(tools) {
//!     eprintln!("{}", e);
//! }
//! ```

use anyhow::{bail, Result};
use std::process::Command;

/// Check if a command exists on the host system.
///
/// Uses `which` to locate the command in PATH.
pub fn command_exists(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Required host tools for building distribution ISOs.
///
/// Each tuple is (command_name, package_name).
pub const REQUIRED_TOOLS: &[(&str, &str)] = &[
    ("mksquashfs", "squashfs-tools"),
    ("xorriso", "xorriso"),
    ("mkfs.fat", "dosfstools"),
    ("mmd", "mtools"),
    ("mcopy", "mtools"),
    ("cpio", "cpio"),
    ("gzip", "gzip"),
];

/// Check that specific tools are available.
///
/// # Arguments
///
/// * `tools` - Slice of (command, package) tuples
///
/// # Returns
///
/// * `Ok(())` if all tools are found
/// * `Err` with list of missing tools and their packages
pub fn check_required_tools(tools: &[(&str, &str)]) -> Result<()> {
    let mut missing = Vec::new();

    for (tool, package) in tools {
        if !command_exists(tool) {
            missing.push((*tool, *package));
        }
    }

    if !missing.is_empty() {
        let msg = missing
            .iter()
            .map(|(t, p)| format!("  {} (install: {})", t, p))
            .collect::<Vec<_>>()
            .join("\n");
        bail!("Missing required host tools:\n{}", msg);
    }

    Ok(())
}

/// Check that all standard ISO-building tools are available.
///
/// This checks all tools in [`REQUIRED_TOOLS`].
pub fn check_host_tools() -> Result<()> {
    check_required_tools(REQUIRED_TOOLS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_exists() {
        // 'ls' should exist on any Unix system
        assert!(command_exists("ls"));
        // Random garbage should not exist
        assert!(!command_exists("definitely_not_a_real_command_12345"));
    }

    #[test]
    fn test_check_required_tools_success() {
        // These should exist on any Unix system
        let tools = &[("ls", "coreutils"), ("cat", "coreutils")];
        assert!(check_required_tools(tools).is_ok());
    }

    #[test]
    fn test_check_required_tools_failure() {
        let tools = &[("nonexistent_command_xyz", "fake-package")];
        assert!(check_required_tools(tools).is_err());
    }
}
