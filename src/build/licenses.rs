//! License tracking and copying for redistributed binaries.
//!
//! Tracks which packages are used during the build and copies their license
//! files from `/usr/share/licenses/<package>/` to the staging directory.
//!
//! Package ownership is determined dynamically by querying the package database
//! in the source rootfs. Supports both RPM and APK package managers.

use anyhow::{Context, Result};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::context::PackageManager;

/// Tracks packages used during the build for license compliance.
///
/// When binaries or libraries are copied, register them with this tracker.
/// After the build completes, call `copy_licenses()` to copy all license files.
pub struct LicenseTracker {
    source: PathBuf,
    pkg_mgr: PackageManager,
    packages: RefCell<HashSet<String>>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl LicenseTracker {
    /// Create a new license tracker.
    ///
    /// `source` is the path to the source rootfs containing the package database.
    /// `pkg_mgr` selects whether to query RPM or APK for file ownership.
    pub fn new(source: PathBuf, pkg_mgr: PackageManager) -> Self {
        Self {
            source,
            pkg_mgr,
            packages: RefCell::new(HashSet::new()),
            cache: RefCell::new(HashMap::new()),
        }
    }

    /// Register a binary that was copied.
    ///
    /// Queries the package database to find which package owns the binary,
    /// searching common binary locations.
    pub fn register_binary(&self, binary: &str) {
        let search_paths = [
            format!("usr/bin/{}", binary),
            format!("usr/sbin/{}", binary),
            format!("usr/lib/systemd/{}", binary),
            format!("usr/lib/udev/{}", binary),
        ];

        for rel_path in &search_paths {
            if let Some(pkg) = self.query_file(rel_path) {
                self.packages.borrow_mut().insert(pkg);
                return;
            }
        }
    }

    /// Register a library that was copied.
    ///
    /// Queries the package database to find which package owns the library,
    /// searching common library locations.
    pub fn register_library(&self, lib: &str) {
        let search_paths = match self.pkg_mgr {
            PackageManager::Rpm => vec![format!("usr/lib64/{}", lib), format!("usr/lib/{}", lib)],
            PackageManager::Apk => vec![format!("usr/lib/{}", lib), format!("lib/{}", lib)],
        };

        for rel_path in &search_paths {
            if let Some(pkg) = self.query_file(rel_path) {
                self.packages.borrow_mut().insert(pkg);
                return;
            }
        }
    }

    /// Register a package directly by name.
    ///
    /// Use this for content that doesn't go through the binary/library mappings,
    /// such as firmware, kernel modules, or data files.
    pub fn register_package(&self, package: &str) {
        self.packages.borrow_mut().insert(package.to_string());
    }

    /// Get the number of packages tracked.
    pub fn package_count(&self) -> usize {
        self.packages.borrow().len()
    }

    /// Query the package database for the package owning a file.
    ///
    /// `rel_path` is relative to the rootfs (e.g. "usr/bin/bash").
    /// Results are cached to avoid repeated subprocess calls.
    fn query_file(&self, rel_path: &str) -> Option<String> {
        // Check cache first
        if let Some(cached) = self.cache.borrow().get(rel_path) {
            return cached.clone();
        }

        let result = match self.pkg_mgr {
            PackageManager::Rpm => self.rpm_query_file(rel_path),
            PackageManager::Apk => self.apk_query_file(rel_path),
        };

        self.cache
            .borrow_mut()
            .insert(rel_path.to_string(), result.clone());
        result
    }

    /// Query RPM database for the package owning a file.
    fn rpm_query_file(&self, rel_path: &str) -> Option<String> {
        let abs_path = format!("/{}", rel_path);
        let output = Command::new("rpm")
            .args([
                "--root",
                self.source.to_str().unwrap_or(""),
                "-qf",
                &abs_path,
                "--queryformat",
                "%{NAME}\n",
            ])
            .output()
            .ok()?;

        if output.status.success() {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !s.contains("not owned"))
        } else {
            None
        }
    }

    /// Query APK database for the package owning a file.
    ///
    /// Parses output like: `/<path> is owned by <pkg>-<ver>`
    /// Also handles: `/<path> symlink target is owned by <pkg>-<ver>`
    fn apk_query_file(&self, rel_path: &str) -> Option<String> {
        let abs_path = format!("/{}", rel_path);
        let output = Command::new("apk")
            .args([
                "info",
                "--root",
                self.source.to_str().unwrap_or(""),
                "-W",
                &abs_path,
            ])
            .output()
            .ok()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Output format: "/<path> is owned by <name>-<version>-r<rev>"
            // Also: "/<path> symlink target is owned by <name>-<version>-r<rev>"
            stdout
                .lines()
                .next()
                .and_then(|line| line.rsplit("is owned by ").next())
                .and_then(strip_apk_version)
                .filter(|s| !s.is_empty())
        } else {
            None
        }
    }

    /// Copy license directories for all used packages.
    ///
    /// Searches for licenses in:
    /// - `source/usr/share/licenses/<pkg>/` (standard location)
    /// - `source/usr/share/doc/<pkg>/` (Alpine fallback — licenses often in -doc subpackages)
    ///
    /// All found licenses are copied to `staging/usr/share/licenses/<pkg>/`.
    /// Returns the number of license directories copied.
    pub fn copy_licenses(&self, source: &Path, staging: &Path) -> Result<usize> {
        let license_dst = staging.join("usr/share/licenses");
        fs::create_dir_all(&license_dst)?;

        // Search paths: primary license dir, then doc dir as fallback
        let search_dirs = [
            source.join("usr/share/licenses"),
            source.join("usr/share/doc"),
        ];

        let packages = self.packages.borrow();
        let mut copied = 0;
        let mut missing = Vec::new();

        for pkg in packages.iter() {
            let dst = license_dst.join(pkg);
            let mut found = false;

            for search_dir in &search_dirs {
                let src = search_dir.join(pkg);
                if src.is_dir() {
                    copy_dir_recursive(&src, &dst)
                        .with_context(|| format!("copying licenses for {}", pkg))?;
                    copied += 1;
                    found = true;
                    break;
                }
            }

            if !found {
                missing.push(pkg.as_str());
            }
        }

        if !missing.is_empty() {
            println!(
                "  Note: {} packages have no license dir: {}",
                missing.len(),
                missing.join(", ")
            );
        }

        Ok(copied)
    }
}

/// Extract the package name from an APK package-version string.
///
/// APK format: `<name>-<version>-r<revision>` (e.g. `busybox-1.36.1-r2`)
///
/// Parses from the right to handle package names containing `-<digit>` sequences
/// (e.g. `font-adobe-75dpi-1.0.3-r2` → `font-adobe-75dpi`).
///
/// Algorithm:
/// 1. Strip the `-rN` revision suffix from the end
/// 2. Find the last `-` followed by a digit — that's where the version starts
/// 3. Everything before that is the package name
fn strip_apk_version(pkg_ver: &str) -> Option<String> {
    let pkg_ver = pkg_ver.trim();

    // Step 1: Strip trailing -rN revision suffix
    // Find the last "-r" followed by only digits to the end
    let without_rev = if let Some(pos) = pkg_ver.rfind("-r") {
        let after_r = &pkg_ver[pos + 2..];
        if !after_r.is_empty() && after_r.chars().all(|c| c.is_ascii_digit()) {
            &pkg_ver[..pos]
        } else {
            pkg_ver
        }
    } else {
        pkg_ver
    };

    // Step 2: Find the last '-' followed by a digit — that's the version start
    let mut name_end = without_rev.len();
    for (i, _) in without_rev.rmatch_indices('-') {
        if let Some(ch) = without_rev[i + 1..].chars().next() {
            if ch.is_ascii_digit() {
                name_end = i;
                break;
            }
        }
    }

    let name = &without_rev[..name_end];
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if src_path.is_symlink() {
            let target = fs::read_link(&src_path)?;
            if !dst_path.exists() && !dst_path.is_symlink() {
                std::os::unix::fs::symlink(&target, &dst_path)?;
            }
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_register_package_directly() {
        let tracker = LicenseTracker::new(PathBuf::from("/nonexistent"), PackageManager::Rpm);
        tracker.register_package("linux-firmware");
        tracker.register_package("tzdata");
        tracker.register_package("kbd");

        assert_eq!(tracker.package_count(), 3);
    }

    #[test]
    fn test_deduplicates_packages() {
        let tracker = LicenseTracker::new(PathBuf::from("/nonexistent"), PackageManager::Rpm);
        tracker.register_package("bash");
        tracker.register_package("bash");
        tracker.register_package("bash");

        assert_eq!(tracker.package_count(), 1);
    }

    #[test]
    fn test_unknown_binaries_ignored() {
        // With a nonexistent rootfs, queries will all fail
        let tracker = LicenseTracker::new(PathBuf::from("/nonexistent"), PackageManager::Rpm);
        tracker.register_binary("nonexistent-binary");

        assert_eq!(tracker.package_count(), 0);
    }

    #[test]
    fn test_apk_tracker_creation() {
        let tracker = LicenseTracker::new(PathBuf::from("/nonexistent"), PackageManager::Apk);
        tracker.register_package("busybox");
        assert_eq!(tracker.package_count(), 1);
    }

    #[test]
    fn test_unknown_binaries_ignored_apk() {
        let tracker = LicenseTracker::new(PathBuf::from("/nonexistent"), PackageManager::Apk);
        tracker.register_binary("nonexistent-binary");
        assert_eq!(tracker.package_count(), 0);
    }

    #[test]
    fn test_strip_apk_version_simple() {
        assert_eq!(
            strip_apk_version("busybox-1.36.1-r2"),
            Some("busybox".into())
        );
    }

    #[test]
    fn test_strip_apk_version_with_subpackage() {
        assert_eq!(
            strip_apk_version("musl-dev-1.2.4-r0"),
            Some("musl-dev".into())
        );
    }

    #[test]
    fn test_strip_apk_version_hyphen_digit_in_name() {
        // Package name contains -75 which is hyphen+digit — must not be mistaken for version
        assert_eq!(
            strip_apk_version("font-adobe-75dpi-1.0.3-r2"),
            Some("font-adobe-75dpi".into())
        );
    }

    #[test]
    fn test_strip_apk_version_100dpi() {
        assert_eq!(
            strip_apk_version("font-adobe-100dpi-1.0.3-r2"),
            Some("font-adobe-100dpi".into())
        );
    }

    #[test]
    fn test_strip_apk_version_musl() {
        assert_eq!(strip_apk_version("musl-1.2.4-r0"), Some("musl".into()));
    }

    #[test]
    fn test_strip_apk_version_openrc() {
        assert_eq!(strip_apk_version("openrc-0.52-r1"), Some("openrc".into()));
    }

    #[test]
    fn test_strip_apk_version_no_revision() {
        // Degenerate case: no -rN suffix (shouldn't happen in practice but be safe)
        assert_eq!(strip_apk_version("busybox-1.36.1"), Some("busybox".into()));
    }

    #[test]
    fn test_strip_apk_version_empty() {
        assert_eq!(strip_apk_version(""), None);
    }

    #[test]
    fn test_strip_apk_version_openssh() {
        assert_eq!(
            strip_apk_version("openssh-server-9.6_p1-r0"),
            Some("openssh-server".into())
        );
    }
}
