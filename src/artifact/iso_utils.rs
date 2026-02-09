//! ISO creation utilities shared between LevitateOS and AcornOS.
//!
//! These functions handle the common parts of ISO creation that are
//! identical regardless of the underlying distribution.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use crate::process::Cmd;
use distro_spec::shared::{
    EFIBOOT_SIZE_MB, ISO_BOOT_DIR, ISO_CHECKSUM_SUFFIX, ISO_EFI_DIR, ISO_LIVE_DIR,
    SHA512_SEPARATOR, XORRISO_FS_FLAGS, XORRISO_PARTITION_OFFSET,
};

/// Create ISO directory structure.
///
/// Creates the standard directory layout for a bootable ISO:
/// - boot/ - kernel, initramfs
/// - live/ - squashfs, overlay
/// - EFI/BOOT/ - UEFI bootloader
///
/// # Arguments
///
/// * `iso_root` - Root directory for ISO contents
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::iso_utils::setup_iso_structure;
/// use std::path::Path;
///
/// setup_iso_structure(Path::new("/tmp/iso-root"))?;
/// ```
pub fn setup_iso_structure(iso_root: &Path) -> Result<()> {
    // Clean previous build
    if iso_root.exists() {
        fs::remove_dir_all(iso_root)?;
    }

    fs::create_dir_all(iso_root.join(ISO_BOOT_DIR))?;
    fs::create_dir_all(iso_root.join(ISO_LIVE_DIR))?;
    fs::create_dir_all(iso_root.join(ISO_EFI_DIR))?;

    Ok(())
}

/// Generate SHA512 checksum for an ISO file.
///
/// Writes checksum in standard format: "<hash>  <filename>" (two spaces)
/// Uses just the filename (not full path) so users can verify with:
///   cd output && sha512sum -c distro.iso.sha512
///
/// # Arguments
///
/// * `iso_path` - Path to the ISO file to checksum
///
/// # Returns
///
/// Path to the generated checksum file (iso_path with .sha512 extension)
pub fn generate_iso_checksum(iso_path: &Path) -> Result<std::path::PathBuf> {
    let result = Cmd::new("sha512sum")
        .arg_path(iso_path)
        .error_msg("sha512sum failed. Install coreutils.")
        .run()?;

    // Extract hash and replace full path with just filename
    // sha512sum outputs: "<hash>  <full_path>"
    // We want: "<hash>  <filename>"
    let hash = result
        .stdout
        .split_whitespace()
        .next()
        .context("Could not parse sha512sum output - no hash found")?;

    let filename = iso_path
        .file_name()
        .context("Could not get ISO filename")?
        .to_string_lossy();

    // Standard format: "<hash>  <filename>" (two spaces between hash and filename)
    let checksum_content = format!("{}{}{}\n", hash, SHA512_SEPARATOR, filename);

    let checksum_path = iso_path.with_extension(ISO_CHECKSUM_SUFFIX.trim_start_matches('.'));
    fs::write(&checksum_path, &checksum_content)?;

    // Print abbreviated hash for visual confirmation
    if hash.len() >= 16 {
        println!("  SHA512: {}...{}", &hash[..8], &hash[hash.len() - 8..]);
    }
    println!("  Wrote: {}", checksum_path.display());

    Ok(checksum_path)
}

/// Create a FAT16 EFI boot image.
///
/// Creates an empty FAT16 image file that can be populated with EFI boot files
/// using mtools (mmd, mcopy).
///
/// # Arguments
///
/// * `output` - Path for the output efiboot.img file
/// * `size_mb` - Size of the image in megabytes (minimum 16 for FAT16)
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::iso_utils::create_fat16_image;
/// use std::path::Path;
///
/// create_fat16_image(Path::new("/tmp/efiboot.img"), 16)?;
/// ```
pub fn create_fat16_image(output: &Path, size_mb: u32) -> Result<()> {
    let output_str = output.to_string_lossy();

    // Create empty file with dd
    Cmd::new("dd")
        .args(["if=/dev/zero", &format!("of={}", output_str)])
        .args(["bs=1M", &format!("count={}", size_mb)])
        .error_msg("Failed to create FAT16 image with dd")
        .run()?;

    // Format as FAT16
    Cmd::new("mkfs.fat")
        .args(["-F", "16"])
        .arg_path(output)
        .error_msg("mkfs.fat failed. Install dosfstools.")
        .run()?;

    Ok(())
}

/// Create EFI directory structure in a FAT image using mtools.
///
/// Creates ::EFI/BOOT directory structure inside the FAT image.
///
/// # Arguments
///
/// * `fat_image` - Path to the FAT16 image file
pub fn create_efi_dirs_in_fat(fat_image: &Path) -> Result<()> {
    let img_str = fat_image.to_string_lossy();

    Cmd::new("mmd")
        .args(["-i", &img_str, "::EFI"])
        .error_msg("mmd failed. Install mtools.")
        .run()?;

    Cmd::new("mmd")
        .args(["-i", &img_str, "::EFI/BOOT"])
        .error_msg("mmd failed to create ::EFI/BOOT directory")
        .run()?;

    Ok(())
}

/// Copy a file into a FAT image using mcopy.
///
/// # Arguments
///
/// * `fat_image` - Path to the FAT16 image file
/// * `src` - Source file to copy
/// * `dst` - Destination path inside the FAT image (e.g., "::EFI/BOOT/")
pub fn mcopy_to_fat(fat_image: &Path, src: &Path, dst: &str) -> Result<()> {
    let img_str = fat_image.to_string_lossy();

    Cmd::new("mcopy")
        .args(["-i", &img_str])
        .arg_path(src)
        .arg(dst)
        .error_msg(format!("mcopy failed to copy {}", src.display()))
        .run()?;

    Ok(())
}

/// Create a complete EFI boot image with bootloader files.
///
/// This is a convenience function that:
/// 1. Creates a FAT16 image
/// 2. Creates EFI/BOOT directory structure
/// 3. Copies the specified EFI files into it
///
/// # Arguments
///
/// * `output` - Path for the output efiboot.img file
/// * `efi_files` - List of (source_path, filename) pairs to copy into ::EFI/BOOT/
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::iso_utils::create_efi_boot_image;
/// use std::path::Path;
///
/// create_efi_boot_image(
///     Path::new("/tmp/efiboot.img"),
///     &[
///         (Path::new("/tmp/iso/EFI/BOOT/BOOTX64.EFI"), "BOOTX64.EFI"),
///         (Path::new("/tmp/iso/EFI/BOOT/grubx64.efi"), "grubx64.efi"),
///         (Path::new("/tmp/iso/EFI/BOOT/grub.cfg"), "grub.cfg"),
///     ],
/// )?;
/// ```
pub fn create_efi_boot_image(output: &Path, efi_files: &[(&Path, &str)]) -> Result<()> {
    // Create FAT16 image
    create_fat16_image(output, EFIBOOT_SIZE_MB)?;

    // Create directory structure
    create_efi_dirs_in_fat(output)?;

    // Copy each EFI file
    for (src, _name) in efi_files {
        mcopy_to_fat(output, src, "::EFI/BOOT/")?;
    }

    Ok(())
}

/// Run xorriso to create a bootable ISO.
///
/// Creates a hybrid UEFI-bootable ISO using xorriso with standard options.
///
/// # Arguments
///
/// * `iso_root` - Directory containing ISO contents
/// * `output` - Path for the output ISO file
/// * `label` - Volume label (used for boot device detection)
/// * `efiboot_filename` - Name of the EFI boot image file in iso_root
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::iso_utils::run_xorriso;
/// use std::path::Path;
///
/// run_xorriso(
///     Path::new("/tmp/iso-root"),
///     Path::new("/tmp/distro.iso"),
///     "MYDISTRO",
///     "efiboot.img",
/// )?;
/// ```
pub fn run_xorriso(
    iso_root: &Path,
    output: &Path,
    label: &str,
    efiboot_filename: &str,
) -> Result<()> {
    Cmd::new("xorriso")
        .args(["-as", "mkisofs", "-o"])
        .arg_path(output)
        .args(["-V", label]) // Volume label for device detection
        .args(["-partition_offset", &XORRISO_PARTITION_OFFSET.to_string()])
        .args(XORRISO_FS_FLAGS)
        .args([
            "-e",
            efiboot_filename,
            "-no-emul-boot",
            "-isohybrid-gpt-basdat",
        ])
        .arg_path(iso_root)
        .error_msg("xorriso failed. Install xorriso.")
        .run()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_setup_iso_structure() {
        let temp = TempDir::new().unwrap();
        let iso_root = temp.path().join("iso-root");

        setup_iso_structure(&iso_root).unwrap();

        assert!(iso_root.join("boot").exists());
        assert!(iso_root.join("live").exists());
        assert!(iso_root.join("EFI/BOOT").exists());
    }
}
