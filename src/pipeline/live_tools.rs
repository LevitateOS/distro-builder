//! The workspace tools the products carry, built by
//! Recipe (static musl; only the TUI binary follows the libc), each at `usr/bin/<binary>`.
use anyhow::{Context, Result};
use std::path::Path;

/// Where a live tool goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolLayer {
    /// The overlay of a product with the install screen: on its live medium
    /// only, never on a system it installs.
    Overlay,
    /// The system rootfs (test-base-rootfs): on every product and every system
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

/// The install tools, on the live medium only. recab is one of them: an A/B
/// install's boot step (`recsetup --boot ab`) puts it and its `recab-started`
/// service into the target, and a mutable install never gets it. Then recipe,
/// the package manager of every system, and recguest in the rootfs; and the
/// install screen's split pane.
pub(crate) const LIVE_TOOLS: &[LiveTool] = &[
    tool("recpart", None, ToolLayer::Overlay),
    tool("recstrap", None, ToolLayer::Overlay),
    tool("recstrap", Some("recfstab"), ToolLayer::Overlay),
    tool("recstrap", Some("recchroot"), ToolLayer::Overlay),
    tool("recstrap", Some("recsetup"), ToolLayer::Overlay),
    tool("recab", None, ToolLayer::Overlay),
    tool("recguest", None, ToolLayer::Rootfs),
    tool("levitate-recipe", None, ToolLayer::Rootfs),
    tool("install-split-pane", Some("install-split"), ToolLayer::Overlay),
];

/// Builds the tools of `layer` into `root`, the layer's tree: test-base-rootfs
/// takes the rootfs tools, test-install-base' overlay the others.
pub(crate) fn add_tools(repo: &Path, root: &Path, layer: ToolLayer, distro_id: &str) -> Result<()> {
    for tool in LIVE_TOOLS.iter().filter(|tool| tool.layer == layer) {
        let tree = crate::recipe::tools::build_tool_tree(repo, tool.package, tool.binary)
            .with_context(|| format!("preparing {} for {distro_id}", tool.package))?;
        levitate_recipe::target::merge_tree(&tree, root)?;
    }
    Ok(())
}
