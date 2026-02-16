//! Shared contract interfaces used by distro builders.

pub mod component;
pub mod context;
pub mod disk;
pub mod kernel;

pub use component::{Installable, Op, Phase};
pub use context::{BuildContext, DistroConfig, InitSystem, PackageManager};
pub use disk::{DiskImageConfig, DiskUuids};
pub use kernel::KernelInstallConfig;
