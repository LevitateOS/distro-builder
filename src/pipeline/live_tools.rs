//! Select Recipe-produced package trees for the live or installed layer.
use anyhow::{Context, Result};
use distro_contract::{Profile, RuntimeActionContract};
use std::path::Path;

pub(crate) fn add_required_tools(
    repo: &Path,
    rootfs: &Path,
    overlay: &Path,
    distro_id: &str,
    profile: &Profile,
    actions: &[RuntimeActionContract],
) -> Result<()> {
    for action in actions {
        let (package, binary, destination, layer) = match action {
            RuntimeActionContract::ToolPayloadWorkspaceBinary { package, binary } => {
                (package, binary, None, overlay)
            }
            RuntimeActionContract::RootfsWorkspaceBinary {
                package,
                binary,
                destination,
            } => (package, binary, Some(destination.as_str()), rootfs),
        };
        let tree = crate::recipe::tools::build_tool_tree(
            repo,
            package,
            binary.as_deref(),
            destination,
            profile,
        )
        .with_context(|| format!("preparing {package} for {distro_id}"))?;
        levitate_recipe::target::merge_tree(&tree, layer)?;
    }
    Ok(())
}
