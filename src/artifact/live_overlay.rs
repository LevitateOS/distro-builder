//! The init system selected by the product contract. Live filesystem policy is
//! implemented by recipes/live-overlay.rhai, separate from the installed rootfs.
#[derive(Debug, Clone, Copy)]
pub enum InitSystem {
    OpenRc,
    Systemd,
}
