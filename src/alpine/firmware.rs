//! Firmware custom operations.
//!
//! Copies WiFi and other hardware firmware from the source rootfs.

use anyhow::Result;
use std::fs;
use std::path::Path;

use super::context::BuildContext;

/// WiFi firmware directories to copy.
///
/// These cover the most common WiFi chipsets:
/// - Intel: iwlwifi (most common on laptops)
/// - Realtek: rtlwifi, rtw88, rtw89
/// - Atheros: ath9k, ath10k, ath11k
/// - MediaTek: mediatek (mt76xx)
/// - Broadcom: brcm
const WIFI_FIRMWARE_DIRS: &[&str] = &[
    // Intel
    "iwlwifi",
    // Realtek
    "rtlwifi",
    "rtl_bt",
    "rtl_nic",
    "rtw88",
    "rtw89",
    // Atheros
    "ath9k_htc",
    "ath10k",
    "ath11k",
    // MediaTek
    "mediatek",
    // Broadcom
    "brcm",
    // Marvell
    "mrvl",
    // Ralink
    "rt2870.bin",
    "rt3070.bin",
    "rt3290.bin",
];

/// Copy WiFi firmware from source to staging.
///
/// This copies only the WiFi firmware needed for most laptops.
/// For a daily driver, this is essential - many laptops have no
/// Ethernet port and depend entirely on WiFi.
pub fn copy_wifi_firmware(ctx: &BuildContext) -> Result<()> {
    let fw_src = ctx.source.join("lib/firmware");
    let fw_dst = ctx.staging.join("lib/firmware");

    if !fw_src.exists() {
        println!("  Warning: firmware source not found, skipping WiFi firmware");
        return Ok(());
    }

    fs::create_dir_all(&fw_dst)?;

    let mut copied_dirs = 0;
    let mut total_size: u64 = 0;

    for dir_name in WIFI_FIRMWARE_DIRS {
        let src = fw_src.join(dir_name);
        if src.exists() {
            let dst = fw_dst.join(dir_name);
            let size = copy_firmware_tree(&src, &dst)?;
            if size > 0 {
                copied_dirs += 1;
                total_size += size;
            }
        }
    }

    println!(
        "  Copied {} WiFi firmware directories ({:.1} MB)",
        copied_dirs,
        total_size as f64 / 1024.0 / 1024.0
    );

    Ok(())
}

/// Copy all firmware from source to staging.
///
/// For a daily driver desktop, we need full hardware support.
/// This includes:
/// - GPU firmware (amdgpu, i915, nvidia)
/// - Sound firmware (sof)
/// - WiFi and Bluetooth
/// - NIC firmware
pub fn copy_all_firmware(ctx: &BuildContext) -> Result<()> {
    let fw_src = ctx.source.join("lib/firmware");
    let fw_dst = ctx.staging.join("lib/firmware");

    if !fw_src.exists() {
        println!(
            "  Warning: firmware source not found at {}",
            fw_src.display()
        );
        return Ok(());
    }

    // Copy the entire firmware directory
    let size = copy_firmware_tree(&fw_src, &fw_dst)?;

    println!(
        "  Copied all firmware ({:.1} MB)",
        size as f64 / 1024.0 / 1024.0
    );

    Ok(())
}

/// Copy a firmware directory tree, tracking size.
fn copy_firmware_tree(src: &Path, dst: &Path) -> Result<u64> {
    let mut total_size: u64 = 0;

    if !src.exists() {
        return Ok(0);
    }

    if src.is_file() {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        let metadata = fs::metadata(src)?;
        total_size = metadata.len();
        fs::copy(src, dst)?;
        return Ok(total_size);
    }

    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_symlink() {
            let target = fs::read_link(&src_path)?;
            if !dst_path.exists() && !dst_path.is_symlink() {
                std::os::unix::fs::symlink(&target, &dst_path)?;
            }
        } else if src_path.is_dir() {
            total_size += copy_firmware_tree(&src_path, &dst_path)?;
        } else if !dst_path.exists() {
            let metadata = fs::metadata(&src_path)?;
            total_size += metadata.len();
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(total_size)
}
