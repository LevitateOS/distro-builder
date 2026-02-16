//! Disk image building contracts.

use anyhow::Result;
use std::path::Path;

/// UUIDs for disk image partitions.
#[derive(Debug, Clone)]
pub struct DiskUuids {
    /// Filesystem UUID for root partition (ext4)
    pub root_fs_uuid: String,
    /// Filesystem UUID for EFI partition (vfat serial)
    pub efi_fs_uuid: String,
    /// GPT partition UUID for root partition (used in boot entry)
    pub root_part_uuid: String,
}

/// Distro-specific configuration for disk image building.
pub trait DiskImageConfig {
    /// Hostname to write to /etc/hostname.
    fn hostname(&self) -> &str;

    /// Boot entry filename (e.g., "iuppiter.conf").
    fn boot_entry_filename(&self) -> &str;

    /// Boot entry content for systemd-boot (loader/entries/*.conf).
    fn boot_entry_content(&self, partuuid: &str) -> String;

    /// Loader config content (loader/loader.conf).
    fn loader_config_content(&self) -> String;

    /// Path to kernel image.
    fn kernel_path(&self) -> &Path;

    /// Path to initramfs for installed system.
    fn initramfs_path(&self) -> &Path;

    /// Path to systemd-boot EFI binary.
    fn bootloader_efi_path(&self) -> &Path;

    /// EFI partition size in MB.
    fn efi_size_mb(&self) -> u64;

    /// Disk image size in GB (sparse).
    fn disk_size_gb(&self) -> u32;

    /// Output filename for the raw disk image.
    fn output_filename(&self) -> &str;

    /// Prepare the rootfs for disk installation.
    /// Called after copying rootfs-staging to work dir.
    /// Distro implements: fstab, services, hostname, passwords, etc.
    fn prepare_rootfs(&self, rootfs: &Path, uuids: &DiskUuids) -> Result<()>;

    /// Additional host tools required beyond the base set.
    /// Each entry is (tool_name, package_name).
    fn extra_required_tools(&self) -> Vec<(&str, &str)> {
        vec![]
    }
}
