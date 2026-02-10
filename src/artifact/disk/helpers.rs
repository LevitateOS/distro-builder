//! UUID generation and host tool verification helpers for disk image building.

use crate::process::Cmd;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub use distro_contract::disk::DiskUuids;

/// Generate new random UUIDs for a disk image.
pub fn generate_disk_uuids() -> Result<DiskUuids> {
    Ok(DiskUuids {
        root_fs_uuid: generate_uuid()?,
        efi_fs_uuid: generate_vfat_serial()?,
        root_part_uuid: generate_uuid()?,
    })
}

/// Base host tools required for disk image building (without qemu-img).
const BASE_REQUIRED_TOOLS: &[(&str, &str)] = &[
    ("sfdisk", "util-linux"),
    ("mkfs.vfat", "dosfstools"),
    ("mkfs.ext4", "e2fsprogs"),
    ("mcopy", "mtools"),
    ("mmd", "mtools"),
    ("uuidgen", "util-linux"),
    ("dd", "coreutils"),
];

/// Verify all required host tools are available.
///
/// Checks base tools plus any extras specified by the caller.
pub fn check_host_tools(extra_tools: &[(&str, &str)]) -> Result<()> {
    let mut missing = Vec::new();

    for (tool, package) in BASE_REQUIRED_TOOLS.iter().chain(extra_tools.iter()) {
        let result = Cmd::new("which").arg(tool).allow_fail().run();
        if result.is_err() || !result.unwrap().success() {
            missing.push(format!("  {} (install: {})", tool, package));
        }
    }

    if !missing.is_empty() {
        bail!(
            "Missing required tools:\n{}\n\nInstall them first.",
            missing.join("\n")
        );
    }

    Ok(())
}

/// Generate a random UUID using uuidgen.
pub fn generate_uuid() -> Result<String> {
    let output = Command::new("uuidgen")
        .output()
        .context("Failed to run uuidgen")?;

    if !output.status.success() {
        bail!("uuidgen failed");
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_lowercase())
}

/// Generate a random FAT32 volume serial (8 hex chars, e.g., "ABCD-1234").
pub fn generate_vfat_serial() -> Result<String> {
    let output = Command::new("uuidgen")
        .output()
        .context("Failed to run uuidgen")?;

    if !output.status.success() {
        bail!("uuidgen failed");
    }

    // Take first 8 hex chars and format as XXXX-XXXX
    let uuid = String::from_utf8_lossy(&output.stdout);
    let hex: String = uuid
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(8)
        .collect();
    if hex.len() < 8 {
        bail!("Failed to generate vfat serial");
    }
    Ok(format!(
        "{}-{}",
        &hex[0..4].to_uppercase(),
        &hex[4..8].to_uppercase()
    ))
}

/// Calculate total size of a directory (including all subdirectories).
/// Returns size in bytes.
pub fn calculate_dir_size(path: &Path) -> Result<u64> {
    let mut total = 0;

    if !path.exists() {
        return Ok(0);
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_dir() {
            total += calculate_dir_size(&entry.path())?;
        } else {
            total += metadata.len();
        }
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_required_tools_list() {
        assert!(!BASE_REQUIRED_TOOLS.is_empty());
        for (tool, package) in BASE_REQUIRED_TOOLS {
            assert!(!tool.is_empty());
            assert!(!package.is_empty());
        }
    }

    #[test]
    fn test_generate_vfat_serial_format() {
        let serial = generate_vfat_serial().unwrap();
        assert_eq!(serial.len(), 9); // XXXX-XXXX
        assert_eq!(&serial[4..5], "-");
    }
}
