use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Install the canonical runtime scenario scripts and their shared libraries.
pub fn install_scenario_test_scripts(repo_root: &Path, rootfs_source_dir: &Path) -> Result<()> {
    let scripts_src = repo_root.join("testing/install-tests/test-scripts");
    if !scripts_src.is_dir() {
        bail!(
            "scenario test scripts source directory not found: '{}'",
            scripts_src.display()
        );
    }

    let bin_dst = rootfs_source_dir.join("usr/local/bin");
    let lib_dst = rootfs_source_dir.join("usr/local/lib/scenario-tests");
    fs::create_dir_all(&bin_dst)
        .with_context(|| format!("creating scenario scripts bin dir '{}'", bin_dst.display()))?;
    fs::create_dir_all(&lib_dst)
        .with_context(|| format!("creating scenario scripts lib dir '{}'", lib_dst.display()))?;

    let entries = fs::read_dir(&scripts_src)
        .with_context(|| format!("reading scenario scripts dir '{}'", scripts_src.display()))?;
    for entry in entries {
        let entry = entry
            .with_context(|| format!("reading directory entry in '{}'", scripts_src.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("reading file type for '{}'", entry.path().display()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let source = entry.path();

        if file_type.is_file() && name.ends_with(".sh") {
            let dest = bin_dst.join(name.as_ref());
            fs::copy(&source, &dest).with_context(|| {
                format!(
                    "copying scenario script '{}' to '{}'",
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
            "scenario test common library not found: '{}'",
            common_src.display()
        );
    }
    for entry in fs::read_dir(scripts_src.join("lib")).context("reading scenario test libraries")? {
        let entry = entry.context("reading scenario test library entry")?;
        let source = entry.path();
        if !source.is_file() {
            continue;
        }
        let destination = lib_dst.join(entry.file_name());
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "copying scenario test library '{}' to '{}'",
                source.display(),
                destination.display()
            )
        })?;
        let mut permissions = fs::metadata(&destination)?.permissions();
        permissions.set_mode(0o644);
        fs::set_permissions(&destination, permissions)
            .with_context(|| format!("setting permissions '{}'", destination.display()))?;
    }

    Ok(())
}
