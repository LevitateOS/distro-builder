//! The live install-UX surface the live-tools ISO provides and the install
//! tests check. Paths are relative to the rootfs.

/// The install entrypoint; `--probe` prints `PROBE_PREFIX<helper>`.
pub const INSTALL_ENTRYPOINT: &str = "usr/local/bin/levitate-install-entrypoint";
/// Login profile hook that starts the install UX.
pub const INSTALL_UX_PROFILE: &str = "etc/profile.d/30-install-ux.sh";
/// The install docs command; the split helper runs it
/// in its right pane.
pub const INSTALL_DOCS: &str = "usr/local/bin/levitate-install-docs";
/// The plain-text install docs `INSTALL_DOCS` shows.
pub const INSTALL_DOCS_TEXT: &str = "usr/local/share/levitate/install-docs.txt";
/// The split-pane install helper.
pub const SPLIT_HELPER: &str = "levitate-install-docs-split";
pub const PROBE_PREFIX: &str = "install-entrypoint-helper=";
/// Set to 1, the entrypoint runs the split helper's `--smoke` instead.
pub const SMOKE_ENV: &str = "LEVITATE_INSTALL_ENTRYPOINT_SMOKE";
/// Printed by `levitate-install-docs-split --smoke` (install-split-pane).
pub const SPLIT_SMOKE_OK: &str = "split-smoke:ok";

/// The live `/etc/issue` login hint; the install tests treat it as the live
/// serial console being ready.
pub const LIVE_LOGIN_HINT: &str = "Login as 'root' (no password)";
