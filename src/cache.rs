//! Build caching - hash-based rebuild detection.
//!
//! Uses SHA256 hashes to detect actual content changes, not just mtimes.
//! This prevents unnecessary rebuilds when files are touched but unchanged.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Compute SHA256 hash of a file's contents.
pub fn hash_file(path: &Path) -> Result<String> {
    let content = fs::read(path)
        .with_context(|| format!("Failed to read file for hashing: {}", path.display()))?;
    let hash = Sha256::digest(&content);
    Ok(format!("{:x}", hash))
}

/// Compute SHA256 hash of multiple files concatenated.
/// Returns None if any file doesn't exist.
/// Logs a warning if a file exists but can't be read.
pub fn hash_files(paths: &[&Path]) -> Option<String> {
    let mut hasher = Sha256::new();
    for path in paths {
        if !path.exists() {
            return None;
        }
        match fs::read(path) {
            Ok(content) => hasher.update(&content),
            Err(e) => {
                eprintln!(
                    "  [WARN] Failed to read {} for hashing: {} (cache will be invalidated)",
                    path.display(),
                    e
                );
                return None;
            }
        }
    }
    Some(format!("{:x}", hasher.finalize()))
}

/// Read cached hash from a .hash file.
/// Returns None if file doesn't exist.
/// Logs a warning if file exists but can't be read.
pub fn read_cached_hash(hash_file: &Path) -> Option<String> {
    if !hash_file.exists() {
        return None;
    }
    match fs::read_to_string(hash_file) {
        Ok(s) => Some(s.trim().to_string()),
        Err(e) => {
            eprintln!(
                "  [WARN] Failed to read cache hash file {}: {} (will rebuild)",
                hash_file.display(),
                e
            );
            None
        }
    }
}

/// Write hash to a .hash file.
pub fn write_cached_hash(hash_file: &Path, hash: &str) -> Result<()> {
    if let Some(parent) = hash_file.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(hash_file, hash)?;
    Ok(())
}

/// Check if target needs rebuild based on source hash.
///
/// Returns true if:
/// - Target doesn't exist
/// - Source hash differs from cached hash
///
/// If hash file is missing but target exists, establishes the hash
/// (trusts existing output as valid baseline).
pub fn needs_rebuild(source_hash: &str, hash_file: &Path, target: &Path) -> bool {
    // Target must exist
    if !target.exists() {
        return true;
    }

    // Check cached hash
    match read_cached_hash(hash_file) {
        Some(cached) => cached != source_hash,
        None => {
            // No hash file but output exists - trust it and establish baseline
            let _ = write_cached_hash(hash_file, source_hash);
            false // Skip rebuild, we just established the hash
        }
    }
}

/// Check if a file exists and is newer than another.
pub fn is_newer(source: &Path, target: &Path) -> bool {
    if !target.exists() {
        return true;
    }
    if !source.exists() {
        return false;
    }

    let Ok(src_meta) = source.metadata() else {
        return true;
    };
    let Ok(tgt_meta) = target.metadata() else {
        return true;
    };
    let Ok(src_time) = src_meta.modified() else {
        return true;
    };
    let Ok(tgt_time) = tgt_meta.modified() else {
        return true;
    };

    src_time > tgt_time
}
