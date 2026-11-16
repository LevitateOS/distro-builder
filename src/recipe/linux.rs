//! Selecting the kernel bundle a kernel recipe published.

use super::RecipeResult;
use anyhow::Result;
use std::path::Path;

/// Select the bundle a kernel recipe run published (its one output, `bundle`)
/// as `destination` (the variant's kernel output directory). Existing physical
/// artifact trees are retained, never overwritten. The kernel and modules
/// inside the bundle are checked by the caller (`pipeline::kernel`).
pub fn select_kernel_bundle(result: &RecipeResult, destination: &Path) -> Result<()> {
    let bundle = result.directory("bundle")?;
    super::outputs::select_directory(&bundle, destination)
}
