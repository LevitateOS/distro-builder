//! Declarative component system for building distribution images.
//!
//! Components are defined as data structures that describe WHAT needs
//! to happen, not HOW. An executor interprets these definitions.
//!
//! # Distro-Agnostic Design
//!
//! This module contains ONLY the distro-agnostic parts:
//! - [`Installable`] trait - anything that can be installed
//! - [`Phase`] enum - build ordering
//! - Generic [`Op`] variants - directory, file, symlink, user/group
//!
//! Distro-specific operations (systemd units, OpenRC services) are
//! defined in the distro crates (leviso, AcornOS).
//!
//! # Example
//!
//! ```rust
//! use distro_builder::component::{Installable, Op, Phase};
//!
//! struct MyComponent;
//!
//! impl Installable for MyComponent {
//!     fn name(&self) -> &str { "MyComponent" }
//!     fn phase(&self) -> Phase { Phase::Config }
//!     fn ops(&self) -> Vec<Op> {
//!         vec![
//!             Op::Dir("etc/myapp".into()),
//!             Op::WriteFile("etc/myapp/config".into(), "key=value\n".into()),
//!         ]
//!     }
//! }
//! ```

use std::fmt;

/// Trait for anything that can be installed by an executor.
///
/// Both static component definitions and dynamic service definitions
/// implement this trait.
pub trait Installable {
    /// Name for logging and identification.
    fn name(&self) -> &str;

    /// Build phase for ordering. Components are sorted by phase
    /// before execution to ensure dependencies are satisfied.
    fn phase(&self) -> Phase;

    /// Generate the operations to perform.
    fn ops(&self) -> Vec<Op>;
}

/// Build phases determine component ordering.
///
/// Components are sorted by phase before execution to ensure
/// dependencies are satisfied (e.g., directories exist before
/// files are copied into them).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Phase {
    /// Create FHS directories and merged-usr symlinks.
    Filesystem = 1,
    /// Copy shells, coreutils, essential binaries.
    Binaries = 2,
    /// Init system setup (systemd for LevitateOS, OpenRC for AcornOS).
    Init = 3,
    /// Message bus (dbus).
    MessageBus = 4,
    /// System services (network, time, ssh).
    Services = 5,
    /// /etc configuration files.
    Config = 6,
    /// Package manager, bootloader tools.
    Packages = 7,
    /// Firmware and hardware support.
    Firmware = 8,
    /// Final cleanup and setup.
    Final = 9,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Phase::Filesystem => write!(f, "Filesystem"),
            Phase::Binaries => write!(f, "Binaries"),
            Phase::Init => write!(f, "Init"),
            Phase::MessageBus => write!(f, "MessageBus"),
            Phase::Services => write!(f, "Services"),
            Phase::Config => write!(f, "Config"),
            Phase::Packages => write!(f, "Packages"),
            Phase::Firmware => write!(f, "Firmware"),
            Phase::Final => write!(f, "Final"),
        }
    }
}

/// Generic operations that work across distributions.
///
/// These operations are distro-agnostic. Distro-specific operations
/// (like systemd unit enabling or OpenRC service setup) should use
/// the [`Op::Custom`] variant or be defined in distro-specific crates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    // ─────────────────────────────────────────────────────────────────────
    // Directory operations
    // ─────────────────────────────────────────────────────────────────────
    /// Create a directory (uses create_dir_all).
    Dir(String),

    /// Create a directory with specific permissions (mode as octal, e.g., 0o755).
    DirMode(String, u32),

    /// Create multiple directories at once.
    Dirs(Vec<String>),

    // ─────────────────────────────────────────────────────────────────────
    // File operations
    // ─────────────────────────────────────────────────────────────────────
    /// Write a file with given content.
    WriteFile(String, String),

    /// Write a file with specific permissions (mode as octal).
    WriteFileMode(String, String, u32),

    /// Create a symlink (link_path, target).
    Symlink(String, String),

    /// Copy a single file from source to staging.
    CopyFile(String),

    /// Copy a directory tree from source to staging.
    CopyTree(String),

    // ─────────────────────────────────────────────────────────────────────
    // User/group operations
    // ─────────────────────────────────────────────────────────────────────
    /// Ensure a user exists in /etc/passwd.
    User {
        name: String,
        uid: u32,
        gid: u32,
        home: String,
        shell: String,
    },

    /// Ensure a group exists in /etc/group.
    Group { name: String, gid: u32 },

    // ─────────────────────────────────────────────────────────────────────
    // Binary operations
    // ─────────────────────────────────────────────────────────────────────
    /// Copy a binary with library dependencies to /usr/bin.
    Bin(String),

    /// Copy a binary with library dependencies to /usr/sbin.
    Sbin(String),

    /// Copy multiple binaries to /usr/bin.
    Bins(Vec<String>),

    /// Copy multiple binaries to /usr/sbin.
    Sbins(Vec<String>),

    // ─────────────────────────────────────────────────────────────────────
    // Extension point for distro-specific operations
    // ─────────────────────────────────────────────────────────────────────
    /// Distro-specific custom operation.
    ///
    /// The executor must handle this based on the distro being built.
    /// For LevitateOS: systemd units, udev helpers, etc.
    /// For AcornOS: OpenRC services, busybox applets, etc.
    Custom(String),
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper functions for readable component definitions
// ─────────────────────────────────────────────────────────────────────────────

/// Create a directory.
pub fn dir(path: impl Into<String>) -> Op {
    Op::Dir(path.into())
}

/// Create a directory with specific mode.
pub fn dir_mode(path: impl Into<String>, mode: u32) -> Op {
    Op::DirMode(path.into(), mode)
}

/// Create multiple directories.
pub fn dirs(paths: impl IntoIterator<Item = impl Into<String>>) -> Op {
    Op::Dirs(paths.into_iter().map(|p| p.into()).collect())
}

/// Write a file.
pub fn write_file(path: impl Into<String>, content: impl Into<String>) -> Op {
    Op::WriteFile(path.into(), content.into())
}

/// Write a file with permissions.
pub fn write_file_mode(path: impl Into<String>, content: impl Into<String>, mode: u32) -> Op {
    Op::WriteFileMode(path.into(), content.into(), mode)
}

/// Create a symlink.
pub fn symlink(link: impl Into<String>, target: impl Into<String>) -> Op {
    Op::Symlink(link.into(), target.into())
}

/// Copy a binary to /usr/bin.
pub fn bin(name: impl Into<String>) -> Op {
    Op::Bin(name.into())
}

/// Copy a binary to /usr/sbin.
pub fn sbin(name: impl Into<String>) -> Op {
    Op::Sbin(name.into())
}

/// Copy multiple binaries to /usr/bin.
pub fn bins(names: impl IntoIterator<Item = impl Into<String>>) -> Op {
    Op::Bins(names.into_iter().map(|n| n.into()).collect())
}

/// Copy multiple binaries to /usr/sbin.
pub fn sbins(names: impl IntoIterator<Item = impl Into<String>>) -> Op {
    Op::Sbins(names.into_iter().map(|n| n.into()).collect())
}

/// Custom distro-specific operation.
pub fn custom(name: impl Into<String>) -> Op {
    Op::Custom(name.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_ordering() {
        assert!(Phase::Filesystem < Phase::Binaries);
        assert!(Phase::Binaries < Phase::Init);
        assert!(Phase::Init < Phase::Services);
        assert!(Phase::Services < Phase::Final);
    }

    #[test]
    fn test_op_helpers() {
        assert_eq!(dir("etc/foo"), Op::Dir("etc/foo".into()));
        assert_eq!(
            dir_mode("etc/foo", 0o755),
            Op::DirMode("etc/foo".into(), 0o755)
        );
        assert_eq!(
            write_file("etc/foo", "bar"),
            Op::WriteFile("etc/foo".into(), "bar".into())
        );
    }

    #[test]
    fn test_installable_trait_implementation() {
        /// Mock component for testing trait implementation.
        struct TestComponent;

        impl Installable for TestComponent {
            fn name(&self) -> &str {
                "test-component"
            }

            fn phase(&self) -> Phase {
                Phase::Config
            }

            fn ops(&self) -> Vec<Op> {
                vec![
                    Op::Dir("etc/test".into()),
                    Op::WriteFile("etc/test/config".into(), "test=true".into()),
                ]
            }
        }

        let component = TestComponent;
        assert_eq!(component.name(), "test-component");
        assert_eq!(component.phase(), Phase::Config);
        assert_eq!(component.ops().len(), 2);
    }

    #[test]
    fn test_op_enum_variants() {
        // Verify all Op variants can be constructed
        let ops = vec![
            Op::Dir("etc".into()),
            Op::DirMode("etc".into(), 0o755),
            Op::Dirs(vec!["etc".into(), "var".into()]),
            Op::WriteFile("etc/config".into(), "value".into()),
            Op::WriteFileMode("etc/config".into(), "value".into(), 0o644),
            Op::Symlink("bin".into(), "usr/bin".into()),
            Op::CopyFile("etc/template".into()),
            Op::CopyTree("usr".into()),
            Op::Bin("ls".into()),
            Op::Sbin("init".into()),
            Op::Bins(vec!["ls".into(), "cat".into()]),
            Op::Sbins(vec!["init".into()]),
            Op::User {
                name: "test".into(),
                uid: 1000,
                gid: 1000,
                home: "/home/test".into(),
                shell: "/bin/sh".into(),
            },
            Op::Group {
                name: "users".into(),
                gid: 100,
            },
            Op::Custom("test-op".into()),
        ];

        assert_eq!(ops.len(), 15);
    }

    #[test]
    fn test_phase_display() {
        assert_eq!(Phase::Filesystem.to_string(), "Filesystem");
        assert_eq!(Phase::Binaries.to_string(), "Binaries");
        assert_eq!(Phase::Init.to_string(), "Init");
        assert_eq!(Phase::MessageBus.to_string(), "MessageBus");
        assert_eq!(Phase::Services.to_string(), "Services");
        assert_eq!(Phase::Config.to_string(), "Config");
        assert_eq!(Phase::Packages.to_string(), "Packages");
        assert_eq!(Phase::Firmware.to_string(), "Firmware");
        assert_eq!(Phase::Final.to_string(), "Final");
    }
}
