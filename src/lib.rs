//! Builds a variant's products (rootfs, overlays, release ISOs) over Recipe.

pub mod artifact;
pub mod install_ux;
pub(crate) mod guest;
pub use pipeline::kernel;
pub(crate) mod pipeline;
pub mod product;
pub(crate) mod tui_binary;
pub mod process;
pub mod recipe;
pub mod run_history;

pub use artifact::filesystem::copy_dir_recursive;
pub use artifact::overlayfs::{build_overlayfs_default, create_overlayfs_erofs};
pub use artifact::rootfs::{build_erofs_default, create_erofs};
pub use pipeline::io::resolve_release_product_rootfs_image_for_distro;
pub use pipeline::planner::{missing_parent_releases, parent_rootfs_image};
pub use pipeline::products::{
    load_base_rootfs_product_spec, load_installed_boot_product_spec, load_live_boot_product_spec,
    load_live_tools_product_spec, materialize_live_boot_source_rootfs, prepare_base_rootfs_product,
    prepare_installed_boot_product, prepare_live_boot_product, prepare_live_tools_product,
    BaseProductLayout, DerivedProductLayout, LiveBootProductSpec, OverlayLayout,
};

// Re-export process utilities
pub use process::{ensure_exists, find_first_existing, Cmd, CommandResult};
