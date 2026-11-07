//! Filesystem helpers for artifact building.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// Copy the tree `src` into `dst`. Symlinks are recreated, replacing whatever
/// is at their destination; regular files are overwritten.
pub fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("creating '{}'", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("reading '{}'", src.display()))? {
        let path = entry?.path();
        let dest = dst.join(path.file_name().expect("read_dir entries have a name"));
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            if fs::symlink_metadata(&dest).is_ok() {
                fs::remove_file(&dest).with_context(|| format!("replacing '{}'", dest.display()))?;
            }
            std::os::unix::fs::symlink(fs::read_link(&path)?, &dest)
                .with_context(|| format!("linking '{}'", dest.display()))?;
        } else if meta.is_dir() {
            copy_dir_recursive(&path, &dest)?;
        } else {
            fs::copy(&path, &dest)
                .with_context(|| format!("copying '{}' to '{}'", path.display(), dest.display()))?;
        }
    }
    Ok(())
}

/// Writes `content` to `path`, creating its directory, with permission bits `mode`.
pub(crate) fn write_file(path: &Path, content: impl AsRef<[u8]>, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("setting the mode of '{}'", path.display()))
}
