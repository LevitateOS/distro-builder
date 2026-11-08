//! The product chain: each product starts from its parent's latest built
//! rootfs, and a release build first builds the parents that have none.

use crate::pipeline::io::{
    release_product_rootfs_exists_for_distro, resolve_release_product_rootfs_image_for_distro,
};
use anyhow::Result;
use distro_contract::{ConformanceContract, ProductId};
use std::path::{Path, PathBuf};

/// The products `product` is built on whose latest release has no rootfs
/// image, oldest first.
pub fn missing_parent_releases(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    product: ProductId,
) -> Result<Vec<ProductId>> {
    let mut missing = Vec::new();
    for parent in product.chain().into_iter().filter(|parent| *parent != product) {
        if !release_product_rootfs_exists_for_distro(
            repo_root,
            distro_id,
            parent,
            &contract.artifacts.rootfs,
        )? {
            missing.push(parent);
        }
    }
    Ok(missing)
}

/// The rootfs image of the latest successful release of `product`'s parent;
/// `None` for a product with no parent.
pub fn parent_rootfs_image(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    product: ProductId,
) -> Result<Option<PathBuf>> {
    product
        .parent()
        .map(|parent| {
            resolve_release_product_rootfs_image_for_distro(
                repo_root,
                distro_id,
                parent,
                &contract.artifacts.rootfs,
            )
        })
        .transpose()
}
