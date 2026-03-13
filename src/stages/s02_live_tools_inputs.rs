use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::pipeline::products::{
    load_live_tools_product_spec, prepare_live_tools_product, DerivedProductLayout,
    LiveToolsProduct, LiveToolsProductSpec, OverlayLayout, ParentRootfsInput,
};

pub type S02LiveToolsInputs = LiveToolsProduct;
pub type S02LiveToolsInputSpec = LiveToolsProductSpec;

fn s02_live_tools_layout() -> DerivedProductLayout {
    DerivedProductLayout {
        rootfs_source_dir: PathBuf::from("s02-rootfs-source"),
        parent_rootfs: ParentRootfsInput {
            release_dir_name: "live-boot".to_string(),
            producer_label: "live-boot".to_string(),
            rootfs_filename: "filesystem.erofs".to_string(),
        },
        live_overlay: OverlayLayout {
            issue_banner_label: "S02 Live Tools".to_string(),
            dir_name: "s02-live-overlay".to_string(),
        },
    }
}

pub fn load_s02_live_tools_input_spec(
    repo_root: &Path,
    variant_dir: &Path,
    distro_id: &str,
) -> Result<S02LiveToolsInputSpec> {
    load_live_tools_product_spec(repo_root, variant_dir, distro_id, s02_live_tools_layout())
}

pub fn prepare_s02_live_tools_inputs(
    spec: &S02LiveToolsInputSpec,
    output_dir: &Path,
) -> Result<S02LiveToolsInputs> {
    prepare_live_tools_product(spec, output_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s02_wrapper_owns_stage_compat_layout() {
        let layout = s02_live_tools_layout();
        assert_eq!(layout.rootfs_source_dir, PathBuf::from("s02-rootfs-source"));
        assert_eq!(layout.parent_rootfs.release_dir_name, "live-boot");
        assert_eq!(layout.parent_rootfs.producer_label, "live-boot");
        assert_eq!(layout.parent_rootfs.rootfs_filename, "filesystem.erofs");
        assert_eq!(layout.live_overlay.issue_banner_label, "S02 Live Tools");
        assert_eq!(layout.live_overlay.dir_name, "s02-live-overlay");
    }
}
