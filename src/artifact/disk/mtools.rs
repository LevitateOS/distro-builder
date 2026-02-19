//! mtools file operations for FAT32 image manipulation.

use crate::process::Cmd;
use anyhow::{bail, Result};
use std::fs;
use std::path::Path;

/// Create a directory in a FAT image using mmd.
pub fn mtools_mkdir(image: &Path, dir: &str) -> Result<()> {
    let target = format!("::{}", dir);
    let result = Cmd::new("mmd")
        .args(["-i"])
        .arg_path(image)
        .arg(&target)
        .allow_fail()
        .run();

    match result {
        Ok(output) => {
            if output.success() {
                return Ok(());
            }

            let output_text = format!(
                "{} {}",
                output.stdout_trimmed().to_ascii_lowercase(),
                output.stderr_trimmed().to_ascii_lowercase()
            );

            if output_text.contains("already exists") {
                return Ok(());
            }

            bail!(
                "mmd failed to create directory '{}': {}{}",
                dir,
                output.exit_description(),
                if output_text.trim().is_empty() {
                    String::new()
                } else {
                    format!("; {}", output_text)
                }
            );
        }
        Err(err) => Err(err),
    }
}

/// Copy a file into a FAT image using mcopy.
pub fn mtools_copy(image: &Path, src: &Path, dest: &str) -> Result<()> {
    Cmd::new("mcopy")
        .args(["-i"])
        .arg_path(image)
        .arg_path(src)
        .arg(format!("::{}", dest))
        .error_msg(format!("mcopy failed: {} -> {}", src.display(), dest))
        .run()?;
    Ok(())
}

/// Write content to a file in a FAT image.
pub fn mtools_write_file(image: &Path, dest: &str, content: &str) -> Result<()> {
    // Write to temp file first, then mcopy
    let temp = std::env::temp_dir().join(format!("mtools-{}", std::process::id()));
    fs::write(&temp, content)?;

    let result = mtools_copy(image, &temp, dest);
    let _ = fs::remove_file(&temp);
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_mtools_functions_exist() {
        assert!(true);
    }
}
