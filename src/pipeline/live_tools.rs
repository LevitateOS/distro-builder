//! The workspace tools the products carry, built for the variant's libc by
//! Recipe, each at `usr/bin/<binary>`.
use anyhow::{Context, Result};
use distro_contract::Profile;
use std::path::Path;

/// Where a live tool goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolLayer {
    /// The live-tools overlay: on the live-tools ISO only.
    Overlay,
    /// The system rootfs (base-rootfs): on every product and every system
    /// an ISO installs.
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
/// recab stages A/B updates with recstrap on the installed system; recipe,
/// the package manager of every system; and the install screen's split pane,
/// which is live-tools only.
pub(crate) const LIVE_TOOLS: &[LiveTool] = &[
    tool("recpart", None, ToolLayer::Overlay),
    tool("recstrap", Some("recfstab"), ToolLayer::Overlay),
    tool("recstrap", Some("recchroot"), ToolLayer::Overlay),
    tool("recstrap", Some("recsetup"), ToolLayer::Overlay),
    tool("recab", None, ToolLayer::Rootfs),
    tool("recstrap", None, ToolLayer::Rootfs),
    tool("levitate-recipe", None, ToolLayer::Rootfs),
    tool("install-split-pane", Some("install-split"), ToolLayer::Overlay),
];

/// Builds the tools of `layer` into `root`, the layer's tree: base-rootfs
/// takes the rootfs tools, live-tools' overlay the others.
pub(crate) fn add_tools(
    repo: &Path,
    root: &Path,
    layer: ToolLayer,
    distro_id: &str,
    profile: &Profile,
) -> Result<()> {
    for tool in LIVE_TOOLS.iter().filter(|tool| tool.layer == layer) {
        let tree = crate::recipe::tools::build_tool_tree(repo, tool.package, tool.binary, profile)
            .with_context(|| format!("preparing {} for {distro_id}", tool.package))?;
        levitate_recipe::target::merge_tree(&tree, root)?;
    }
    Ok(())
}
