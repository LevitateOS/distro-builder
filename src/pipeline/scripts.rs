use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) fn install_stage_test_scripts(repo_root: &Path, rootfs_source_dir: &Path) -> Result<()> {
    let scripts_src = repo_root.join("testing/install-tests/test-scripts");
    if !scripts_src.is_dir() {
        bail!(
            "stage test scripts source directory not found: '{}'",
            scripts_src.display()
        );
    }

    let bin_dst = rootfs_source_dir.join("usr/local/bin");
    let lib_dst = rootfs_source_dir.join("usr/local/lib/stage-tests");
    fs::create_dir_all(&bin_dst)
        .with_context(|| format!("creating stage scripts bin dir '{}'", bin_dst.display()))?;
    fs::create_dir_all(&lib_dst)
        .with_context(|| format!("creating stage scripts lib dir '{}'", lib_dst.display()))?;

    let entries = fs::read_dir(&scripts_src)
        .with_context(|| format!("reading stage scripts dir '{}'", scripts_src.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("reading directory entry in '{}'", scripts_src.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for '{}'", entry.path().display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let source = entry.path();

        if file_type.is_file() && name.starts_with("stage-") && name.ends_with(".sh") {
            let dest = bin_dst.join(name.as_ref());
            fs::copy(&source, &dest).with_context(|| {
                format!(
                    "copying stage script '{}' to '{}'",
                    source.display(),
                    dest.display()
                )
            })?;
            let mut perms = fs::metadata(&dest)
                .with_context(|| format!("reading metadata '{}'", dest.display()))?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)
                .with_context(|| format!("setting permissions '{}'", dest.display()))?;
        }
    }

    let common_src = scripts_src.join("lib/common.sh");
    if !common_src.is_file() {
        bail!(
            "stage test common library not found: '{}'",
            common_src.display()
        );
    }
    let common_dst = lib_dst.join("common.sh");
    fs::copy(&common_src, &common_dst).with_context(|| {
        format!(
            "copying stage test common library '{}' to '{}'",
            common_src.display(),
            common_dst.display()
        )
    })?;
    let mut perms = fs::metadata(&common_dst)
        .with_context(|| format!("reading metadata '{}'", common_dst.display()))?
        .permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&common_dst, perms)
        .with_context(|| format!("setting permissions '{}'", common_dst.display()))?;

    Ok(())
}
