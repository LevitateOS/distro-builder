//! What the builder needs to know about each product besides the registry
//! (`distro_contract::ProductId`): one table, keyed by the product.

pub use distro_contract::ProductId;

/// How the builder lays out and assembles a product. Every product's rootfs
/// is the installable system: base-rootfs builds it, and every other product
/// takes its parent's rootfs image unchanged and adds only its overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductBuild {
    /// The overlay directory beside the product's rootfs.
    pub overlay_dir: &'static str,
    /// The file in the prepared product that holds its rootfs source path.
    pub rootfs_source_pointer: &'static str,
    /// The overlay sets up the live session (services, serial test shell), so
    /// the product's ISO boots with the live kernel command line.
    pub live_session: bool,
    /// The overlay also carries the install tools and the install screen,
    /// which runs on the variant's install consoles.
    pub install_screen: bool,
}

pub fn build(product: ProductId) -> ProductBuild {
    const LIVE: ProductBuild = ProductBuild {
        overlay_dir: "live-overlay",
        rootfs_source_pointer: ".live-rootfs-source.path",
        live_session: true,
        install_screen: false,
    };
    match product {
        ProductId::BaseRootfs => ProductBuild {
            live_session: false,
            ..LIVE
        },
        ProductId::LiveBoot => LIVE,
        ProductId::LiveTools => ProductBuild {
            install_screen: true,
            ..LIVE
        },
        ProductId::InstalledBoot => ProductBuild {
            overlay_dir: "boot-overlay",
            rootfs_source_pointer: ".rootfs-source.path",
            live_session: false,
            install_screen: false,
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
