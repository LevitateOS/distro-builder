//! Path definitions for Alpine-based build artifacts.
//!
//! The actual download and extraction logic is in `deps/alpine.rhai`.
//! This module only defines WHERE things go, not HOW to get them.

use std::path::{Path, PathBuf};

/// Paths used during build.
pub struct ExtractPaths {
    /// Downloads directory
    pub downloads: PathBuf,
    /// Path to the Alpine ISO
    pub iso: PathBuf,
    /// Extracted ISO contents
    pub iso_contents: PathBuf,
    /// Rootfs directory
    pub rootfs: PathBuf,
    /// APK tools directory
    pub apk_tools: PathBuf,
}

impl ExtractPaths {
    /// Create paths relative to the base directory.
    pub fn new(base_dir: &Path) -> Self {
        let downloads = base_dir.join("downloads");
        Self {
            iso: downloads.join("alpine-extended-3.23.2-x86_64.iso"),
            iso_contents: downloads.join("iso-contents"),
            rootfs: downloads.join("rootfs"),
            apk_tools: downloads.join("apk-tools"),
            downloads,
        }
    }
}
