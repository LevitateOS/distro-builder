//! Kernel build and installation contracts.

/// Configuration for kernel installation.
///
/// Implemented by distro-specific configs to customize
/// where and how the kernel is installed.
pub trait KernelInstallConfig {
    /// Path where modules are installed (e.g., "/usr/lib/modules" or "/lib/modules").
    fn module_install_path(&self) -> &str;

    /// Kernel filename in /boot (e.g., "vmlinuz").
    fn kernel_filename(&self) -> &str;
}
