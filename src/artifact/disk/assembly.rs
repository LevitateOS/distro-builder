//! Disk assembly — GPT creation and partition splicing.

use super::helpers::DiskUuids;
use anyhow::{bail, Context, Result};
use crate::process::Cmd;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Sector size in bytes.
const SECTOR_SIZE: u64 = 512;

/// First partition starts at this offset (1MB for GPT + alignment).
const FIRST_PARTITION_OFFSET_SECTORS: u64 = 2048; // 1MB / 512

/// Assemble a raw GPT disk image from partition images.
///
/// Creates a sparse disk file with GPT partition table, then splices
/// the EFI and root partition images at their correct offsets.
pub fn assemble_disk(
    disk_path: &Path,
    efi_image: &Path,
    root_image: &Path,
    disk_size_gb: u32,
    efi_size_mb: u64,
    uuids: &DiskUuids,
) -> Result<()> {
    let disk_size_bytes = (disk_size_gb as u64) * 1024 * 1024 * 1024;

    // Create sparse disk image
    {
        let file = fs::File::create(disk_path)?;
        file.set_len(disk_size_bytes)?;
    }

    // Write GPT partition table via sfdisk
    let efi_size_sectors = (efi_size_mb * 1024 * 1024) / SECTOR_SIZE;
    let root_start_sector = FIRST_PARTITION_OFFSET_SECTORS + efi_size_sectors;
    let sfdisk_script = format!(
        "label: gpt\n\
         start={}, size={}, type=U, bootable\n\
         start={}, type=L, uuid={}\n",
        FIRST_PARTITION_OFFSET_SECTORS,
        efi_size_sectors,
        root_start_sector,
        uuids.root_part_uuid.to_uppercase()
    );

    let mut child = Command::new("sfdisk")
        .arg(disk_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Failed to run sfdisk")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(sfdisk_script.as_bytes())?;
    }

    let status = child.wait()?;
    if !status.success() {
        bail!("sfdisk failed to create partition table");
    }

    // Calculate partition offsets
    let efi_offset_bytes = FIRST_PARTITION_OFFSET_SECTORS * SECTOR_SIZE;
    let root_offset_sectors = FIRST_PARTITION_OFFSET_SECTORS + efi_size_sectors;
    let root_offset_bytes = root_offset_sectors * SECTOR_SIZE;

    // Copy EFI partition image into disk
    println!("  Writing EFI partition at offset {}...", efi_offset_bytes);
    Cmd::new("dd")
        .arg(format!("if={}", efi_image.display()))
        .arg(format!("of={}", disk_path.display()))
        .args(["bs=1M", "conv=notrunc"])
        .arg(format!("seek={}", efi_offset_bytes / (1024 * 1024)))
        .error_msg("dd failed for EFI partition")
        .run()?;

    // Copy root partition image into disk
    println!(
        "  Writing root partition at offset {}...",
        root_offset_bytes
    );
    Cmd::new("dd")
        .arg(format!("if={}", root_image.display()))
        .arg(format!("of={}", disk_path.display()))
        .args(["bs=1M", "conv=notrunc"])
        .arg(format!("seek={}", root_offset_bytes / (1024 * 1024)))
        .error_msg("dd failed for root partition")
        .run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_constants() {
        assert_eq!(SECTOR_SIZE, 512);
        assert_eq!(FIRST_PARTITION_OFFSET_SECTORS, 2048);
    }
}
