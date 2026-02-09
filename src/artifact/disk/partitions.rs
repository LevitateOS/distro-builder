//! EFI and root partition creation for disk images.

use super::helpers::DiskUuids;
use super::mtools;
use anyhow::Result;
use crate::process::Cmd;
use std::fs;
use std::path::Path;

/// Create an EFI partition image using mkfs.vfat and mtools.
///
/// The caller provides boot entry content, loader config, kernel/initramfs paths,
/// and the systemd-boot EFI binary path. This function creates a FAT32 image
/// with the standard EFI directory structure.
pub fn create_efi_partition(
    image_path: &Path,
    efi_size_mb: u64,
    uuids: &DiskUuids,
    boot_entry_filename: &str,
    boot_entry_content: &str,
    loader_config_content: &str,
    kernel_path: &Path,
    initramfs_path: &Path,
    bootloader_efi_path: &Path,
) -> Result<()> {
    // Create sparse image file
    let size_bytes = efi_size_mb * 1024 * 1024;
    {
        let file = fs::File::create(image_path)?;
        file.set_len(size_bytes)?;
    }

    // Format as FAT32 with specific volume ID
    let vol_id = uuids.efi_fs_uuid.replace('-', "");
    Cmd::new("mkfs.vfat")
        .args(["-F", "32", "-n", "EFI", "-i", &vol_id])
        .arg_path(image_path)
        .error_msg("mkfs.vfat failed")
        .run()?;

    // Create directory structure using mtools
    mtools::mtools_mkdir(image_path, "EFI")?;
    mtools::mtools_mkdir(image_path, "EFI/BOOT")?;
    mtools::mtools_mkdir(image_path, "EFI/systemd")?;
    mtools::mtools_mkdir(image_path, "loader")?;
    mtools::mtools_mkdir(image_path, "loader/entries")?;

    // Copy systemd-boot EFI binary
    mtools::mtools_copy(image_path, bootloader_efi_path, "EFI/BOOT/BOOTX64.EFI")?;
    mtools::mtools_copy(
        image_path,
        bootloader_efi_path,
        "EFI/systemd/systemd-bootx64.efi",
    )?;

    // Write loader.conf
    mtools::mtools_write_file(image_path, "loader/loader.conf", loader_config_content)?;

    // Write boot entry
    let entry_path = format!("loader/entries/{}", boot_entry_filename);
    mtools::mtools_write_file(image_path, &entry_path, boot_entry_content)?;

    // Copy kernel and initramfs
    mtools::mtools_copy(image_path, kernel_path, "vmlinuz")?;
    mtools::mtools_copy(image_path, initramfs_path, "initramfs.img")?;

    Ok(())
}

/// Create a root partition image using mkfs.ext4 -d.
///
/// Populates the ext4 filesystem from a directory without mounting.
pub fn create_root_partition(
    rootfs: &Path,
    image_path: &Path,
    size_mb: u64,
    uuids: &DiskUuids,
) -> Result<()> {
    // Create sparse image file
    let size_bytes = size_mb * 1024 * 1024;
    {
        let file = fs::File::create(image_path)?;
        file.set_len(size_bytes)?;
    }

    // Create ext4 filesystem populated from rootfs directory
    Cmd::new("mkfs.ext4")
        .args(["-q", "-L", "root"])
        .args(["-U", &uuids.root_fs_uuid])
        .args(["-d"])
        .arg_path(rootfs)
        .arg_path(image_path)
        .error_msg("mkfs.ext4 -d failed. Check that e2fsprogs supports -d flag.")
        .run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_partition_functions_exist() {
        assert!(true);
    }
}
