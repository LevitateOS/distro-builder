//! Builds a variant's products over Recipe: cached parts (system trees and
//! images, overlays; `parts`) and the combiner that writes a release ISO from
//! them (`release`).

pub mod artifact;
pub mod cli;
pub mod install_ux;
pub(crate) mod guest;
pub use pipeline::kernel;
pub(crate) mod parts;
pub(crate) mod pipeline;
pub mod product;
pub mod release;
pub mod release_manifest;
pub mod release_index;
pub mod tui_apps;
pub(crate) mod tui_binary;
pub(crate) mod desktop_apps;
pub mod prebuilt;
pub mod process;
pub mod timeline;
pub mod recipe;

pub use artifact::filesystem::copy_dir_recursive;
pub use artifact::overlayfs::build_overlayfs_default;
pub use artifact::rootfs::{build_erofs_default, create_erofs};
pub use pipeline::sources::system_sources_digest;
pub use pipeline::products::{prepare_product, PreparedProduct};
pub use pipeline::source::{apps_build_preseed_recipe, materialize_rootfs_source, preseed_recipe};
