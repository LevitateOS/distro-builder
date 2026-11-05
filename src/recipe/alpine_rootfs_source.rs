use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AlpineRootfsSourcePreseedOutput {
    pub iso_path: PathBuf,
    pub apk_tools_path: PathBuf,
}

pub fn preseed_alpine_rootfs_source_assets(
    repo_root: &Path,
    distro_id: &str,
    refresh: bool,
) -> Result<AlpineRootfsSourcePreseedOutput> {
    let mut outputs = super::rootfs_source::preseed_rootfs_source(
        repo_root,
        distro_id,
        Path::new("distro-builder/recipes/alpine-preseed-source-assets.rhai"),
        &std::collections::BTreeMap::from([
            ("iso".into(), distro_contract::RecipeOutputKind::File),
            ("apk_tools".into(), distro_contract::RecipeOutputKind::File),
        ]),
        refresh,
    )?;
    Ok(AlpineRootfsSourcePreseedOutput {
        iso_path: outputs
            .remove("iso")
            .context("preseed did not report iso")?,
        apk_tools_path: outputs
            .remove("apk_tools")
            .context("preseed did not report apk_tools")?,
    })
}
