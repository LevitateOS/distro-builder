//! ISO image builder.
//!
//! Provides a wrapper around `xorriso` for creating bootable ISO images.
//!
//! # Status
//!
//! This is a placeholder module. The actual ISO building logic
//! remains in leviso. This module defines the interface for future extraction.

use anyhow::Result;
use std::path::Path;

/// Options for building an ISO image.
#[derive(Debug, Clone)]
pub struct IsoOptions<'a> {
    /// Volume label (used for boot device detection via root=LABEL=X).
    pub label: &'a str,

    /// OS name for GRUB menu entries.
    pub os_name: &'a str,

    /// Additional kernel command line options.
    pub cmdline: &'a str,

    /// Whether to create a UEFI-bootable ISO.
    ///
    /// Default: true
    pub uefi: bool,

    /// Whether to create a BIOS-bootable ISO (hybrid).
    ///
    /// Default: false (UEFI-only for modern systems)
    pub bios: bool,
}

impl<'a> IsoOptions<'a> {
    /// Create options for a UEFI-only ISO.
    pub fn uefi_only(label: &'a str, os_name: &'a str) -> Self {
        Self {
            label,
            os_name,
            cmdline: "",
            uefi: true,
            bios: false,
        }
    }

    /// Create options for a hybrid UEFI+BIOS ISO.
    pub fn hybrid(label: &'a str, os_name: &'a str) -> Self {
        Self {
            label,
            os_name,
            cmdline: "",
            uefi: true,
            bios: true,
        }
    }
}

/// Build a UEFI-bootable ISO image.
///
/// # Arguments
///
/// * `iso_root` - Directory containing ISO contents (boot/, live/, EFI/)
/// * `output` - Path for the output ISO file
/// * `options` - ISO build options
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::iso::{build_iso, IsoOptions};
/// use std::path::Path;
///
/// let options = IsoOptions::uefi_only("MYDISTRO", "MyDistro Linux");
///
/// build_iso(
///     Path::new("iso-root/"),
///     Path::new("output/mydistro.iso"),
///     &options,
/// )?;
/// ```
///
/// # Status
///
/// **UNIMPLEMENTED** - This is a placeholder. The actual implementation is in leviso.
pub fn build_iso(_iso_root: &Path, _output: &Path, _options: &IsoOptions) -> Result<()> {
    unimplemented!(
        "ISO building not yet extracted from leviso.\n\
         \n\
         To use ISO building, use leviso directly or wait for\n\
         this functionality to be extracted."
    )
}
