//! Shared disk image builder infrastructure.
//!
//! Provides a trait-based approach for building raw GPT disk images
//! without requiring root privileges. Distros implement `DiskImageConfig`
//! to customize rootfs preparation, boot entries, and services.
//!
//! Used by both leviso (LevitateOS → qcow2) and IuppiterOS (→ raw .img).

pub mod assembly;
pub mod helpers;
pub mod mtools;
pub mod partitions;

pub use crate::contracts::disk::DiskImageConfig;
pub use helpers::{generate_disk_uuids, DiskUuids};

use crate::process::Cmd;
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// Build a raw disk image using the provided config.
///
/// Returns path to the output .img file.
pub fn build_disk_image(
    config: &dyn DiskImageConfig,
    staging_dir: &Path,
    output_dir: &Path,
    work_dir: &Path,
) -> Result<PathBuf> {
    let uuids = helpers::generate_disk_uuids()?;
    build_disk_image_with_uuids(config, staging_dir, output_dir, work_dir, uuids)
}

/// Build a raw disk image using pre-generated UUIDs.
///
/// Use this when the initramfs needs to be built with the PARTUUID baked in
/// before the disk image is assembled.
pub fn build_disk_image_with_uuids(
    config: &dyn DiskImageConfig,
    staging_dir: &Path,
    output_dir: &Path,
    work_dir: &Path,
    uuids: DiskUuids,
) -> Result<PathBuf> {
    println!("=== Building Disk Image (sudo-free) ===\n");

    // Step 1: Check host tools
    println!("Checking host tools...");
    let extra = config.extra_required_tools();
    helpers::check_host_tools(&extra)?;

    // Step 2: Print UUIDs
    println!("Partition UUIDs:");
    println!("  Root FS UUID: {}", uuids.root_fs_uuid);
    println!("  EFI FS UUID:  {}", uuids.efi_fs_uuid);
    println!("  Root PARTUUID: {}", uuids.root_part_uuid);

    // Step 3: Create work directory
    if work_dir.exists() {
        fs::remove_dir_all(work_dir)?;
    }
    fs::create_dir_all(work_dir)?;

    // Step 4: Copy staging to work dir and prepare rootfs
    println!("\nPreparing rootfs...");
    let rootfs_work = work_dir.join("rootfs");
    Cmd::new("cp")
        .args(["-a"])
        .arg_path(staging_dir)
        .arg_path(&rootfs_work)
        .error_msg("Failed to copy rootfs-staging")
        .run()?;

    config
        .prepare_rootfs(&rootfs_work, &uuids)
        .context("Failed to prepare rootfs for disk image")?;

    // Step 5: Create EFI partition
    println!("\nCreating EFI partition image...");
    let efi_image = work_dir.join("efi.img");
    let efi_size_mb = config.efi_size_mb();
    let boot_entry_content = config.boot_entry_content(&uuids.root_part_uuid);
    let loader_config = config.loader_config_content();

    partitions::create_efi_partition(
        &efi_image,
        efi_size_mb,
        &uuids,
        config.boot_entry_filename(),
        &boot_entry_content,
        &loader_config,
        config.kernel_path(),
        config.initramfs_path(),
        config.bootloader_efi_path(),
    )?;

    if let Ok(meta) = fs::metadata(&efi_image) {
        println!("  EFI partition size: {} MB", meta.len() / 1024 / 1024);
    }

    // Step 6: Create root partition
    println!("\nCreating root partition image...");
    let root_image = work_dir.join("root.img");
    let disk_size_gb = config.disk_size_gb();
    let root_size_mb = (disk_size_gb as u64 * 1024) - efi_size_mb - 2; // 2MB for GPT overhead
    partitions::create_root_partition(&rootfs_work, &root_image, root_size_mb, &uuids)?;

    if let Ok(meta) = fs::metadata(&root_image) {
        println!(
            "  Root partition size: {} MB (sparse file)",
            meta.len() / 1024 / 1024
        );
    }

    // Step 7: Assemble GPT disk image
    println!("\nAssembling disk image...");
    let raw_path = work_dir.join("disk.raw");
    assembly::assemble_disk(
        &raw_path,
        &efi_image,
        &root_image,
        disk_size_gb,
        efi_size_mb,
        &uuids,
    )?;

    // Step 8: Move to output
    let output_path = output_dir.join(config.output_filename());
    fs::create_dir_all(output_dir)?;
    if output_path.exists() {
        fs::remove_file(&output_path)?;
    }
    fs::rename(&raw_path, &output_path)
        .or_else(|_| {
            // Cross-filesystem: copy then remove
            fs::copy(&raw_path, &output_path)?;
            fs::remove_file(&raw_path)?;
            Ok::<(), std::io::Error>(())
        })
        .context("Failed to move disk image to output")?;

    // Step 9: Cleanup work directory
    println!("Cleaning up...");
    fs::remove_dir_all(work_dir)?;

    println!("\n=== Disk Image Built ===");
    println!("  Output: {}", output_path.display());
    if let Ok(meta) = fs::metadata(&output_path) {
        println!("  Size: {} MB (sparse)", meta.len() / 1024 / 1024);
    }

    Ok(output_path)
}
