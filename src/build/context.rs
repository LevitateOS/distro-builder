//! Build context and distro configuration traits.
//!
//! These traits define the interface that distro-specific builders
//! must implement to use the shared infrastructure.

use std::path::{Path, PathBuf};

/// Configuration for a specific distribution.
///
/// Implemented by leviso for LevitateOS and by AcornOS crate for AcornOS.
/// This trait provides distro-specific constants and behavior.
///
/// # Example
///
/// ```rust,ignore
/// use distro_builder::build::context::{DistroConfig, InitSystem};
///
/// struct MyDistroConfig;
///
/// impl DistroConfig for MyDistroConfig {
///     fn os_name(&self) -> &str { "MyDistro" }
///     fn os_id(&self) -> &str { "mydistro" }
///     fn iso_label(&self) -> &str { "MYDISTRO" }
///     fn boot_modules(&self) -> &[&str] { &["erofs", "overlay"] }
///     fn default_shell(&self) -> &str { "/bin/bash" }
///     fn init_system(&self) -> InitSystem { InitSystem::Systemd }
/// }
/// ```
pub trait DistroConfig: crate::build::kernel::KernelInstallConfig {
    /// OS name for display (e.g., "LevitateOS", "AcornOS").
    fn os_name(&self) -> &str;

    /// OS identifier used in paths (e.g., "levitateos", "acornos").
    fn os_id(&self) -> &str;

    /// ISO volume label for boot device detection.
    fn iso_label(&self) -> &str;

    /// Kernel modules required for boot.
    fn boot_modules(&self) -> &[&str];

    /// Default shell for the system.
    fn default_shell(&self) -> &str;

    /// Init system type.
    fn init_system(&self) -> InitSystem;
}

/// Package manager types supported by distro-builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    /// RPM (used by LevitateOS / Rocky Linux)
    Rpm,
    /// APK (used by AcornOS, IuppiterOS / Alpine Linux)
    Apk,
}

/// Init system types supported by distro-builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InitSystem {
    /// systemd (used by LevitateOS)
    Systemd,
    /// OpenRC (used by AcornOS)
    OpenRC,
}

impl std::fmt::Display for InitSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitSystem::Systemd => write!(f, "systemd"),
            InitSystem::OpenRC => write!(f, "OpenRC"),
        }
    }
}

/// Shared context for all build operations.
///
/// This is a trait that distro-specific builders implement
/// to provide paths and configuration.
pub trait BuildContext {
    /// Path to the source rootfs (Rocky rootfs, Alpine rootfs, etc.)
    fn source(&self) -> &Path;

    /// Path to the staging directory (where we build the filesystem)
    fn staging(&self) -> &Path;

    /// Base directory of the builder project
    fn base_dir(&self) -> &Path;

    /// Output directory for build artifacts
    fn output(&self) -> &Path;

    /// Get the distro configuration
    fn config(&self) -> &dyn DistroConfig;
}

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
