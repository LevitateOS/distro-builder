//! Builds a variant's products (rootfs, overlays, release ISOs) over Recipe.

pub mod artifact;
pub mod cli;
pub mod install_ux;
pub(crate) mod guest;
pub use pipeline::kernel;
pub(crate) mod pipeline;
pub mod product;
pub mod release;
pub mod tui_apps;
pub(crate) mod tui_binary;
pub mod process;
pub mod recipe;

pub use artifact::filesystem::copy_dir_recursive;
pub use artifact::overlayfs::build_overlayfs_default;
pub use artifact::rootfs::{build_erofs_default, create_erofs};
pub use pipeline::io::latest_release_rootfs;
pub use pipeline::planner::{missing_parent_releases, parent_rootfs_image};
pub use pipeline::products::{prepare_product, PreparedProduct};
pub use pipeline::source::{materialize_rootfs_source, preseed_recipe};
