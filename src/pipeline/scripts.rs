use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub(crate) fn install_scenario_test_scripts(
    repo_root: &Path,
    rootfs_source_dir: &Path,
) -> Result<()> {
    let scripts_src = repo_root.join("testing/install-tests/test-scripts");
    if !scripts_src.is_dir() {
        bail!(
            "scenario test scripts source directory not found: '{}'",
            scripts_src.display()
        );
    }

    let bin_dst = rootfs_source_dir.join("usr/local/bin");
    let lib_dst = rootfs_source_dir.join("usr/local/lib/stage-tests");
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
    let common_dst = lib_dst.join("common.sh");
    fs::copy(&common_src, &common_dst).with_context(|| {
        format!(
            "copying scenario test common library '{}' to '{}'",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .canonicalize()
            .expect("canonicalize workspace root")
    }

    #[test]
    fn install_scenario_test_scripts_copies_canonical_script_names() {
        let repo_root = workspace_root();
        let rootfs_dir = tempfile::tempdir().expect("rootfs tempdir");
        install_scenario_test_scripts(&repo_root, rootfs_dir.path())
            .expect("install scenario test scripts");

        for script in [
            "live-boot.sh",
            "live-boot-ssh-preflight.sh",
            "live-tools.sh",
            "install.sh",
            "installed-boot.sh",
            "automated-login.sh",
            "installed-tools.sh",
        ] {
            assert!(
                rootfs_dir
                    .path()
                    .join("usr/local/bin")
                    .join(script)
                    .is_file(),
                "expected canonical scenario script '{}' to be installed",
                script
            );
        }
        assert!(
            rootfs_dir
                .path()
                .join("usr/local/lib/stage-tests/common.sh")
                .is_file(),
            "expected shared scenario common library to be installed"
        );
    }
}
