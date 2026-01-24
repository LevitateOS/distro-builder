//! Initramfs builder.
//!
//! Provides utilities for creating initial RAM filesystem archives
//! using busybox and cpio.
//!
//! # Status
//!
//! This is a placeholder module. The actual initramfs building logic
//! remains in leviso. This module defines the interface for future extraction.

use anyhow::Result;
use std::path::Path;

/// Options for building an initramfs.
#[derive(Debug, Clone)]
pub struct InitramfsOptions<'a> {
    /// Busybox commands to symlink (e.g., ["sh", "mount", "switch_root"]).
    pub busybox_commands: &'a [&'a str],

    /// Boot modules to include (paths relative to /lib/modules/<version>/).
    pub boot_modules: &'a [&'a str],

    /// Gzip compression level (1-9).
    ///
    /// Higher = smaller file, slower compression.
    /// Default: 6
    pub gzip_level: u8,
}

impl Default for InitramfsOptions<'_> {
    fn default() -> Self {
        Self {
            busybox_commands: &[],
            boot_modules: &[],
            gzip_level: 6,
        }
    }
}

/// Standard busybox commands needed for boot.
pub const STANDARD_BUSYBOX_COMMANDS: &[&str] = &[
    "sh",
    "mount",
    "umount",
    "mkdir",
    "cat",
    "ls",
    "sleep",
    "switch_root",
    "echo",
    "test",
    "[",
    "grep",
    "sed",
    "ln",
    "rm",
    "cp",
    "mv",
    "chmod",
    "chown",
    "mknod",
    "losetup",
    "insmod",
    "modprobe",
];

/// Build an initramfs from components.
///
/// # Arguments
///
/// * `output_dir` - Directory to create initramfs in
/// * `options` - Initramfs build options
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::artifact::initramfs::{build_initramfs, InitramfsOptions, STANDARD_BUSYBOX_COMMANDS};
/// use std::path::Path;
///
/// let options = InitramfsOptions {
///     busybox_commands: STANDARD_BUSYBOX_COMMANDS,
///     boot_modules: &["squashfs", "overlay", "loop"],
///     gzip_level: 9,
/// };
///
/// build_initramfs(Path::new("output/"), &options)?;
/// ```
///
/// # Status
///
/// **UNIMPLEMENTED** - This is a placeholder. The actual implementation is in leviso.
pub fn build_initramfs(_output_dir: &Path, _options: &InitramfsOptions) -> Result<()> {
    unimplemented!(
        "Initramfs building not yet extracted from leviso.\n\
         \n\
         To use initramfs building, use leviso directly or wait for\n\
         this functionality to be extracted."
    )
}
