use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::pipeline::products::{
    load_base_rootfs_product_spec, load_live_boot_product_spec,
    materialize_live_boot_source_rootfs, prepare_base_rootfs_product, prepare_live_boot_product,
    BaseProductLayout, BaseRootfsProduct, BaseRootfsProductSpec, DerivedProductLayout,
    LiveBootProduct, LiveBootProductSpec, OverlayLayout, ParentRootfsInput,
};

pub type S00BuildInputs = BaseRootfsProduct;
pub type S00BuildInputSpec = BaseRootfsProductSpec;
pub type S01BootInputs = LiveBootProduct;
pub type S01BootInputSpec = LiveBootProductSpec;

fn s00_build_layout() -> BaseProductLayout {
    BaseProductLayout {
        rootfs_source_dir: PathBuf::from("s00-rootfs-source"),
        live_overlay_dir_name: "s00-live-overlay".to_string(),
    }
}

fn s01_boot_layout() -> DerivedProductLayout {
    DerivedProductLayout {
        rootfs_source_dir: PathBuf::from("s01-rootfs-source"),
        parent_rootfs: ParentRootfsInput {
            release_dir_name: "base-rootfs".to_string(),
            producer_label: "base-rootfs".to_string(),
            rootfs_filename: "filesystem.erofs".to_string(),
        },
        live_overlay: OverlayLayout {
            issue_banner_label: "S01 Boot".to_string(),
            dir_name: "s01-live-overlay".to_string(),
        },
    }
}

pub fn load_s00_build_input_spec(
    distro_id: &str,
    os_name: &str,
    os_id: &str,
    output_root: &Path,
) -> Result<S00BuildInputSpec> {
    load_base_rootfs_product_spec(distro_id, os_name, os_id, output_root, s00_build_layout())
}

pub fn load_s01_boot_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S01BootInputSpec> {
    load_live_boot_product_spec(repo_root, variant_dir, distro_id, s01_boot_layout())
}

pub fn materialize_s01_source_rootfs(spec: &S01BootInputSpec) -> Result<PathBuf> {
    materialize_live_boot_source_rootfs(spec)
}

pub fn prepare_s00_build_inputs(
    spec: &S00BuildInputSpec,
    output_dir: &Path,
) -> Result<S00BuildInputs> {
    prepare_base_rootfs_product(spec, output_dir)
}

pub fn prepare_s01_boot_inputs(
    spec: &S01BootInputSpec,
    output_dir: &Path,
) -> Result<S01BootInputs> {
    prepare_live_boot_product(spec, output_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s00_wrapper_owns_stage_compat_layout() {
        let layout = s00_build_layout();
        assert_eq!(layout.rootfs_source_dir, PathBuf::from("s00-rootfs-source"));
        assert_eq!(layout.live_overlay_dir_name, "s00-live-overlay");
    }

    #[test]
    fn s01_wrapper_owns_stage_compat_layout() {
        let layout = s01_boot_layout();
        assert_eq!(layout.rootfs_source_dir, PathBuf::from("s01-rootfs-source"));
        assert_eq!(layout.parent_rootfs.release_dir_name, "base-rootfs");
        assert_eq!(layout.parent_rootfs.producer_label, "base-rootfs");
        assert_eq!(layout.parent_rootfs.rootfs_filename, "filesystem.erofs");
        assert_eq!(layout.live_overlay.issue_banner_label, "S01 Boot");
        assert_eq!(layout.live_overlay.dir_name, "s01-live-overlay");
    }
}
