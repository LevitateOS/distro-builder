//! Select Recipe-produced package trees for the live or installed layer.
use anyhow::{Context, Result};
use distro_contract::RuntimeActionContract;
use std::path::Path;

pub(crate) fn add_required_tools(
    repo: &Path,
    rootfs: &Path,
    overlay: &Path,
    distro_id: &str,
    actions: &[RuntimeActionContract],
) -> Result<()> {
    let init = match distro_id {
        "acorn" => "openrc",
        "levitate" => "systemd",
        _ => anyhow::bail!("unsupported distro {distro_id}"),
    };
    for action in actions {
        let (package, binary, target, destination, layer) = match action {
            RuntimeActionContract::ToolPayloadWorkspaceBinary {
                package,
                binary,
                target,
            } => (package, binary, target, None, overlay),
            RuntimeActionContract::RootfsWorkspaceBinary {
                package,
                binary,
                target,
                destination,
            } => (package, binary, target, Some(destination.as_str()), rootfs),
        };
        let tree = crate::recipe::tools::build_tool_tree(
            repo,
            package,
            binary.as_deref(),
            target.as_deref(),
            destination,
            init,
        )
        .with_context(|| format!("preparing {package} for {distro_id}"))?;
        levitate_recipe::target::merge_tree(&tree, layer)?;
    }
    Ok(())
}
