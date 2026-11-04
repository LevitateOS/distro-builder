//! Alpine signing key setup and verification.
//!
//! This module handles copying Alpine signing keys into the rootfs
//! at `/etc/apk/keys/` so that APK can verify package signatures during installation.
//!
//! Keys are passed as parameters to remain distro-agnostic within the Alpine family.

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::Path;

/// Install Alpine signing keys into the rootfs.
///
/// # Arguments
/// * `rootfs_path` - Path to the rootfs where keys should be installed
/// * `keys` - Slice of (filename, content) tuples for signing keys
pub fn install_keys(rootfs_path: &Path, keys: &[(&str, &str)]) -> Result<()> {
    let keys_dir = rootfs_path.join("etc/apk/keys");

    // Ensure the keys directory exists
    fs::create_dir_all(&keys_dir)
        .with_context(|| format!("Failed to create keys directory: {}", keys_dir.display()))?;

    // Install each key
    for (filename, content) in keys {
        let key_path = keys_dir.join(filename);

        fs::write(&key_path, content)
            .with_context(|| format!("Failed to write key file: {}", key_path.display()))?;

        // Verify the key was written correctly
        if !key_path.exists() {
            bail!("Key file was not written: {}", key_path.display());
        }

        // Verify key content is valid (contains PEM header)
        let written_content = fs::read_to_string(&key_path)
            .with_context(|| format!("Failed to read back key file: {}", key_path.display()))?;

        if !written_content.contains("BEGIN PUBLIC KEY") {
            bail!(
                "Key file {} does not contain valid PEM format",
                key_path.display()
            );
        }
    }

    println!("  Alpine signing keys installed ({} keys)", keys.len());

    Ok(())
}

/// Verify that Alpine signing keys are properly installed in the rootfs.
///
/// # Arguments
/// * `rootfs_path` - Path to the rootfs to verify
/// * `keys` - Slice of (filename, content) tuples for expected signing keys
pub fn verify_keys(rootfs_path: &Path, keys: &[(&str, &str)]) -> Result<()> {
    let keys_dir = rootfs_path.join("etc/apk/keys");

    if !keys_dir.exists() {
        bail!(
            "Alpine keys directory does not exist: {}",
            keys_dir.display()
        );
    }

    // Verify each expected key file exists
    for (filename, _) in keys {
        let key_path = keys_dir.join(filename);

        if !key_path.exists() {
            bail!("Alpine signing key missing: {}", key_path.display());
        }

        // Verify it's readable and contains valid PEM format
        let content = fs::read_to_string(&key_path)
            .with_context(|| format!("Failed to read key file: {}", key_path.display()))?;

        if !content.contains("BEGIN PUBLIC KEY") {
            bail!(
                "Key file {} does not contain valid PEM format",
                key_path.display()
            );
        }
    }

    println!("  Alpine signing keys verified ({} keys)", keys.len());

    Ok(())
}
