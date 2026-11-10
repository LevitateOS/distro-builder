//! The product chain: each product starts from its parent's latest built
//! rootfs, and a release build first builds the parents that have none or
//! whose system was built from older sources than the checkout's.

use crate::pipeline::io::latest_release_rootfs;
use crate::pipeline::sources;
use anyhow::Result;
use distro_contract::{ConformanceContract, ProductId};
use std::path::{Path, PathBuf};

/// The products `product` is built on that must be built first, oldest
/// first: a parent whose latest release has no rootfs image, or whose rootfs
/// was built from other sources than the checkout has now
/// (`sources::system_sources_digest`), and every product after such a parent
/// in the chain, since it carries that parent's rootfs.
pub fn parent_releases_to_build(
    repo_root: &Path,
    distro_id: &str,
    contract: &ConformanceContract,
    product: ProductId,
) -> Result<Vec<ProductId>> {
    let current = sources::system_sources_digest(repo_root, distro_id)?;
    let mut build = Vec::new();
    for parent in product.chain().into_iter().filter(|parent| *parent != product) {
        let current_release = latest_release_rootfs(repo_root, distro_id, parent, &contract.artifacts.rootfs)?
            .and_then(|image| image.parent().and_then(sources::recorded))
            .is_some_and(|recorded| recorded == current);
        if !build.is_empty() || !current_release {
            build.push(parent);
        }
    }
    Ok(build)
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
            latest_release_rootfs(repo_root, distro_id, parent, &contract.artifacts.rootfs)?
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{distro_id} has no successful {parent} release with a rootfs image ({}); build that release first",
                        contract.artifacts.rootfs
                    )
                })
        })
        .transpose()
}
