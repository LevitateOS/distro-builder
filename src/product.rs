//! What the builder needs to know about each product besides the registry
//! (`distro_contract::ProductId`): one table, keyed by the product.

pub use distro_contract::ProductId;

/// How the builder lays out and assembles a product.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductBuild {
    /// The overlay directory beside the product's rootfs.
    pub overlay_dir: &'static str,
    /// The file in the prepared product that holds its rootfs source path.
    pub rootfs_source_pointer: &'static str,
    /// The overlay sets up the live session (services, serial test shell), so
    /// the product's ISO boots with the live kernel command line.
    pub live_session: bool,
    /// The product's rootfs is the whole installable system, so its ISO
    /// carries the installed initramfs built from it. base-rootfs is not.
    pub carries_installed_system: bool,
}

pub fn build(product: ProductId) -> ProductBuild {
    const LIVE: ProductBuild = ProductBuild {
        overlay_dir: "live-overlay",
        rootfs_source_pointer: ".live-rootfs-source.path",
        live_session: true,
        carries_installed_system: true,
    };
    match product {
        ProductId::BaseRootfs => ProductBuild {
            live_session: false,
            carries_installed_system: false,
            ..LIVE
        },
        ProductId::LiveBoot | ProductId::LiveTools => LIVE,
        ProductId::InstalledBoot => ProductBuild {
            overlay_dir: "boot-overlay",
            rootfs_source_pointer: ".rootfs-source.path",
            live_session: false,
            carries_installed_system: true,
        },
    }
}

/// A product that is built into a release ISO, with the variant hook that
/// assembles it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReleaseProduct {
    pub product: ProductId,
    /// The script under `distro/variants/<id>/hooks/`.
    pub hook: &'static str,
}

impl ReleaseProduct {
    /// `None` unless `product.is_release()`.
    pub fn new(product: ProductId) -> Option<Self> {
        let hook = match product {
            ProductId::BaseRootfs => "build-release.sh",
            ProductId::LiveBoot => "boot-release.sh",
            ProductId::LiveTools => "live-tools-release.sh",
            ProductId::InstalledBoot => return None,
        };
        debug_assert!(product.is_release());
        Some(Self { product, hook })
    }

    pub fn all() -> impl Iterator<Item = ReleaseProduct> {
        ProductId::releases().filter_map(Self::new)
    }
}
