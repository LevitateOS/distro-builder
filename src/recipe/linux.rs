//! Selecting the kernel bundle a kernel recipe published.

use super::RecipeResult;
use anyhow::Result;
use std::path::Path;

/// Select the bundle a kernel recipe run published as `destination` (the
/// variant's kernel output directory). Existing physical artifact trees are
/// retained, never overwritten.
pub fn select_kernel_bundle(ctx: &RecipeResult, destination: &Path) -> Result<()> {
    let bundle = ctx.directory("bundle")?;
    ctx.file("kernel")?;
    ctx.directory("modules")?;
    super::outputs::select_directory(&bundle, destination)
}
