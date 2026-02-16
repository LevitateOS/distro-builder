//! Build context and distro configuration.
//!
//! Traits and enums are defined in `distro-builder::contracts` and re-exported here.
//! `SimpleBuildContext` is an implementation that lives in distro-builder.

use std::path::PathBuf;

// Re-export contracts from distro-builder contracts module
pub use crate::contracts::context::{BuildContext, DistroConfig, InitSystem, PackageManager};

/// Simple implementation of BuildContext for basic use cases.
///
/// This is useful for testing or simple build scenarios where
/// you don't need the full distro-specific context.
pub struct SimpleBuildContext {
    /// Path to the source rootfs
    pub source: PathBuf,
    /// Path to the staging directory
    pub staging: PathBuf,
    /// Base directory of the builder project
    pub base_dir: PathBuf,
    /// Output directory for build artifacts
    pub output: PathBuf,
}

impl SimpleBuildContext {
    /// Create a new simple build context.
    pub fn new(source: PathBuf, staging: PathBuf, base_dir: PathBuf, output: PathBuf) -> Self {
        Self {
            source,
            staging,
            base_dir,
            output,
        }
    }
}
