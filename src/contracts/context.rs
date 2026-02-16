//! Build context and distro configuration contracts.

use std::path::Path;

use crate::contracts::kernel::KernelInstallConfig;

/// Configuration for a specific distribution.
///
/// Implemented by leviso for LevitateOS, AcornOS, and IuppiterOS.
/// This trait provides distro-specific constants and behavior.
pub trait DistroConfig: KernelInstallConfig {
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
    /// OpenRC (used by AcornOS, IuppiterOS)
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
/// Distro-specific builders implement this to provide paths and configuration.
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
