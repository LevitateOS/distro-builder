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
    /// The overlay sets up the live session (services, serial test shell).
    pub live_session: bool,
    /// The overlay also carries the install tools and the install screen,
    /// which runs on the variant's install consoles.
    pub install_screen: bool,
}

pub fn build(product: ProductId) -> ProductBuild {
    const LIVE: ProductBuild = ProductBuild {
        overlay_dir: "live-overlay",
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
            live_session: false,
            install_screen: false,
        },
    }
}
