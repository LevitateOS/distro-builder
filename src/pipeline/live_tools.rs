//! The workspace tools the live-tools product carries, built for the
//! variant's libc by Recipe, each at `usr/bin/<binary>`.
use anyhow::{Context, Result};
use distro_contract::Profile;
use std::path::Path;

/// Where a live tool goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolLayer {
    /// The live overlay: on the live ISO only.
    Overlay,
    /// The rootfs: also on every system the ISO installs.
    Rootfs,
}

/// A binary of a workspace package (the package's own binary when `binary`
/// is `None`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LiveTool {
    pub(crate) package: &'static str,
    pub(crate) binary: Option<&'static str>,
    pub(crate) layer: ToolLayer,
}

const fn tool(package: &'static str, binary: Option<&'static str>, layer: ToolLayer) -> LiveTool {
    LiveTool {
        package,
        binary,
        layer,
    }
}

/// The install tools; recab and recstrap also go into the rootfs, since
/// recab stages A/B updates with recstrap on the installed system; and the
/// install screen's split pane, which is live only.
pub(crate) const LIVE_TOOLS: &[LiveTool] = &[
    tool("recpart", None, ToolLayer::Overlay),
    tool("recstrap", None, ToolLayer::Overlay),
    tool("recstrap", Some("recfstab"), ToolLayer::Overlay),
    tool("recstrap", Some("recchroot"), ToolLayer::Overlay),
    tool("recstrap", Some("recsetup"), ToolLayer::Overlay),
    tool("recab", None, ToolLayer::Rootfs),
    tool("recstrap", None, ToolLayer::Rootfs),
    tool("install-split-pane", Some("install-split"), ToolLayer::Overlay),
];

/// Whether the live-tools product puts a binary of the Cargo `package` on
/// the live ISO.
pub(crate) fn ships(package: &str) -> bool {
    LIVE_TOOLS.iter().any(|tool| tool.package == package)
}

pub(crate) fn add_required_tools(
    repo: &Path,
    rootfs: &Path,
    overlay: &Path,
    distro_id: &str,
    profile: &Profile,
) -> Result<()> {
    for tool in LIVE_TOOLS {
        let tree = crate::recipe::tools::build_tool_tree(repo, tool.package, tool.binary, profile)
            .with_context(|| format!("preparing {} for {distro_id}", tool.package))?;
        let layer = match tool.layer {
            ToolLayer::Overlay => overlay,
            ToolLayer::Rootfs => rootfs,
        };
        levitate_recipe::target::merge_tree(&tree, layer)?;
    }
    Ok(())
}
