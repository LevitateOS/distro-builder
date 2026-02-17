//! Centralized, repo-local artifact store (content-addressed).
//!
//! Goals:
//! - Store build artifacts in a single place (repo root `/.artifacts/`)
//! - Address blobs by sha256
//! - Provide a small index keyed by an "input key" (typically the existing
//!   `output/.<artifact>-inputs.hash` files) so distros can quickly restore
//!   missing outputs without rebuilding.
//!
//! This is intentionally NOT a package manager. It stores *build outputs* only.

use crate::artifact::filesystem::copy_dir_recursive;
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tar::Builder as TarBuilder;
use walkdir::WalkDir;

/// Default store directory name at repo root.
pub const DEFAULT_STORE_DIR: &str = ".artifacts";

/// Centralized (non-content-addressed) output directory root within the store.
///
/// This directory is intended to replace per-distro `output/` trees (e.g.
/// `leviso/output/`) when working in the superrepo.
pub const DEFAULT_OUTPUT_SUBDIR: &str = "out";

/// Artifact encoding format stored as a blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactFormat {
    /// A single file blob.
    File,
    /// A tar archive compressed with zstd.
    TarZst,
}

/// Index entry mapping an input key to a content-addressed blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub kind: String,
    pub input_key: String,
    pub blob_sha256: String,
    pub format: ArtifactFormat,
    pub size_bytes: u64,
    pub stored_at_unix: u64,
    #[serde(default)]
    pub meta: BTreeMap<String, serde_json::Value>,
}

/// A stored artifact resolved from the index.
#[derive(Debug, Clone)]
pub struct StoredArtifact {
    pub entry: IndexEntry,
    pub blob_path: PathBuf,
}

/// Artifact store rooted at `<repo>/.artifacts`.
#[derive(Debug, Clone)]
pub struct ArtifactStore {
    root: PathBuf,
}

impl ArtifactStore {
    /// Open (and create if needed) the store at `<repo_root>/.artifacts`.
    pub fn open(repo_root: &Path) -> Result<Self> {
        let root = repo_root.join(DEFAULT_STORE_DIR);
        let store = Self { root };
        store.ensure_layout()?;
        Ok(store)
    }

    /// Open the store for a distro crate directory (e.g. `<repo>/AcornOS`).
    pub fn open_for_distro(base_dir: &Path) -> Result<Self> {
        let repo_root = base_dir.parent().unwrap_or(base_dir);
        Self::open(repo_root)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn ensure_layout(&self) -> Result<()> {
        fs::create_dir_all(self.blobs_dir().join("sha256"))?;
        fs::create_dir_all(self.index_dir())?;
        fs::create_dir_all(self.tmp_dir())?;
        fs::create_dir_all(self.locks_dir())?;
        Ok(())
    }

    fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }

    fn index_dir(&self) -> PathBuf {
        self.root.join("index")
    }

    fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    fn kind_dir(&self, kind: &str) -> Result<PathBuf> {
        validate_kind(kind)?;
        Ok(self.index_dir().join(kind))
    }

    fn index_path(&self, kind: &str, input_key: &str) -> Result<PathBuf> {
        validate_kind(kind)?;
        validate_key(input_key)?;
        Ok(self
            .index_dir()
            .join(kind)
            .join(format!("{}.json", input_key)))
    }

    fn lock_path(&self, kind: &str, input_key: &str) -> Result<PathBuf> {
        validate_kind(kind)?;
        validate_key(input_key)?;
        Ok(self
            .locks_dir()
            .join(kind)
            .join(format!("{}.lock", input_key)))
    }

    fn blob_path(&self, sha256: &str) -> Result<PathBuf> {
        validate_sha256(sha256)?;
        let prefix = &sha256[0..2];
        Ok(self.blobs_dir().join("sha256").join(prefix).join(sha256))
    }

    /// Get an artifact from the index if present.
    pub fn get(&self, kind: &str, input_key: &str) -> Result<Option<StoredArtifact>> {
        let index_path = self.index_path(kind, input_key)?;
        if !index_path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&index_path)
            .with_context(|| format!("Failed to read index {}", index_path.display()))?;
        let entry: IndexEntry = serde_json::from_slice(&bytes)
            .with_context(|| format!("Failed to parse index {}", index_path.display()))?;

        let blob_path = self.blob_path(&entry.blob_sha256)?;
        Ok(Some(StoredArtifact { entry, blob_path }))
    }

    /// Store a file artifact as a blob and update the index.
    pub fn put_blob_file(
        &self,
        kind: &str,
        input_key: &str,
        src_file: &Path,
        mut meta: BTreeMap<String, serde_json::Value>,
    ) -> Result<String> {
        if !src_file.exists() {
            bail!("Source file not found: {}", src_file.display());
        }

        let _lock = self.acquire_lock(kind, input_key)?;

        let (sha256, size_bytes) = sha256_file(src_file)?;
        let blob_path = self.blob_path(&sha256)?;

        // Ensure blob directory exists
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write blob if missing
        if !blob_path.exists() {
            let tmp = self
                .tmp_dir()
                .join(tmp_name(&format!("blob-{}", &sha256[..16])));
            fs::copy(src_file, &tmp).with_context(|| {
                format!("Failed to copy {} to {}", src_file.display(), tmp.display())
            })?;
            atomic_rename(&tmp, &blob_path)?;
        }

        meta.insert(
            "source_path".to_string(),
            serde_json::Value::String(src_file.display().to_string()),
        );

        // Write index (atomic)
        let stored_at_unix = now_unix();
        let entry = IndexEntry {
            kind: kind.to_string(),
            input_key: input_key.to_string(),
            blob_sha256: sha256.clone(),
            format: ArtifactFormat::File,
            size_bytes,
            stored_at_unix,
            meta,
        };
        self.write_index(kind, input_key, &entry)?;

        Ok(sha256)
    }

    /// Ingest a file into the store by moving it into the blob path and
    /// hardlinking it back to the original location (when possible).
    ///
    /// This is intended for migration/cleanup workflows where we want the
    /// canonical bytes to live under `.artifacts/` without duplicating disk.
    pub fn ingest_file_move_and_link(
        &self,
        kind: &str,
        input_key: &str,
        src_file: &Path,
        mut meta: BTreeMap<String, serde_json::Value>,
    ) -> Result<String> {
        if !src_file.exists() {
            bail!("Source file not found: {}", src_file.display());
        }

        let _lock = self.acquire_lock(kind, input_key)?;

        let (sha256, size_bytes) = sha256_file(src_file)?;
        let blob_path = self.blob_path(&sha256)?;

        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // If blob does not exist, move the source file into place.
        if !blob_path.exists() {
            let tmp_blob = self
                .tmp_dir()
                .join(tmp_name(&format!("adopt-{}", &sha256[..16])));

            // Try to rename (fast path, same filesystem). If that fails, fall back to copy+remove.
            match fs::rename(src_file, &tmp_blob) {
                Ok(()) => {
                    // tmp_blob lives under the store; now rename into the blob path.
                    atomic_rename(&tmp_blob, &blob_path)?;
                }
                Err(_) => {
                    fs::copy(src_file, &tmp_blob).with_context(|| {
                        format!(
                            "Failed to copy {} to {}",
                            src_file.display(),
                            tmp_blob.display()
                        )
                    })?;
                    atomic_rename(&tmp_blob, &blob_path)?;
                    fs::remove_file(src_file).with_context(|| {
                        format!("Failed to remove source file {}", src_file.display())
                    })?;
                }
            }
        }

        // Ensure the original path exists and points at the blob (hardlink if possible).
        if src_file.exists() {
            let _ = fs::remove_file(src_file);
        }
        if fs::hard_link(&blob_path, src_file).is_err() {
            fs::copy(&blob_path, src_file).with_context(|| {
                format!(
                    "Failed to copy blob {} back to {}",
                    blob_path.display(),
                    src_file.display()
                )
            })?;
        }

        meta.insert(
            "source_path".to_string(),
            serde_json::Value::String(src_file.display().to_string()),
        );

        let entry = IndexEntry {
            kind: kind.to_string(),
            input_key: input_key.to_string(),
            blob_sha256: sha256.clone(),
            format: ArtifactFormat::File,
            size_bytes,
            stored_at_unix: now_unix(),
            meta,
        };
        self.write_index(kind, input_key, &entry)?;

        Ok(sha256)
    }

    /// Store a directory as a deterministic `tar.zst` blob and update the index.
    pub fn put_dir_as_tar_zst(
        &self,
        kind: &str,
        input_key: &str,
        src_dir: &Path,
        mut meta: BTreeMap<String, serde_json::Value>,
    ) -> Result<String> {
        if !src_dir.is_dir() {
            bail!("Source directory not found: {}", src_dir.display());
        }

        let _lock = self.acquire_lock(kind, input_key)?;

        let tmp_tar = self.tmp_dir().join(tmp_name("artifact.tar.zst"));
        create_tar_zst(src_dir, &tmp_tar)?;

        let (sha256, size_bytes) = sha256_file(&tmp_tar)?;
        let blob_path = self.blob_path(&sha256)?;

        // Ensure blob directory exists
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if !blob_path.exists() {
            atomic_rename(&tmp_tar, &blob_path)?;
        } else {
            // Blob already exists; remove tmp.
            let _ = fs::remove_file(&tmp_tar);
        }

        meta.insert(
            "source_path".to_string(),
            serde_json::Value::String(src_dir.display().to_string()),
        );

        let stored_at_unix = now_unix();
        let entry = IndexEntry {
            kind: kind.to_string(),
            input_key: input_key.to_string(),
            blob_sha256: sha256.clone(),
            format: ArtifactFormat::TarZst,
            size_bytes,
            stored_at_unix,
            meta,
        };
        self.write_index(kind, input_key, &entry)?;

        Ok(sha256)
    }

    /// Store the kernel payload (vmlinuz + modules) from a staging directory.
    ///
    /// This stores a `tar.zst` containing:
    /// - `boot/vmlinuz`
    /// - `lib/modules/**` OR `usr/lib/modules/**` (whichever exists)
    ///
    /// The payload is keyed by `input_key` (typically `output/.kernel-inputs.hash`).
    pub fn put_kernel_payload(
        &self,
        input_key: &str,
        staging_dir: &Path,
        mut meta: BTreeMap<String, serde_json::Value>,
    ) -> Result<String> {
        let kind = "kernel_payload";
        validate_key(input_key)?;

        let vmlinuz = staging_dir.join("boot/vmlinuz");
        if !vmlinuz.exists() {
            bail!("Kernel not installed (missing {}):", vmlinuz.display());
        }

        let modules_candidates = [
            staging_dir.join("lib/modules"),
            staging_dir.join("usr/lib/modules"),
        ];
        let modules_dir = modules_candidates
            .iter()
            .find(|p| p.exists())
            .cloned()
            .with_context(|| {
                format!(
                    "Kernel modules not found in staging (checked {} and {})",
                    modules_candidates[0].display(),
                    modules_candidates[1].display()
                )
            })?;

        let _lock = self.acquire_lock(kind, input_key)?;

        // Build a minimal payload directory under the store tmp/ so we can reuse the
        // deterministic tar builder.
        let payload_dir = self.tmp_dir().join(tmp_name("kernel-payload-dir"));
        if payload_dir.exists() {
            let _ = fs::remove_dir_all(&payload_dir);
        }
        fs::create_dir_all(payload_dir.join("boot"))?;
        hardlink_or_copy(&vmlinuz, &payload_dir.join("boot/vmlinuz"))?;

        // Preserve modules path root (lib/modules vs usr/lib/modules).
        let rel_modules = modules_dir
            .strip_prefix(staging_dir)
            .unwrap_or(&modules_dir);
        let dst_modules = payload_dir.join(rel_modules);
        if let Some(parent) = dst_modules.parent() {
            fs::create_dir_all(parent)?;
        }
        copy_dir_recursive(&modules_dir, &dst_modules)?;

        let tmp_tar = self.tmp_dir().join(tmp_name("kernel_payload.tar.zst"));
        create_tar_zst(&payload_dir, &tmp_tar)?;
        let _ = fs::remove_dir_all(&payload_dir);

        let (sha256, size_bytes) = sha256_file(&tmp_tar)?;
        let blob_path = self.blob_path(&sha256)?;
        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if !blob_path.exists() {
            atomic_rename(&tmp_tar, &blob_path)?;
        } else {
            let _ = fs::remove_file(&tmp_tar);
        }

        meta.insert(
            "source_path".to_string(),
            serde_json::Value::String(staging_dir.display().to_string()),
        );

        let entry = IndexEntry {
            kind: kind.to_string(),
            input_key: input_key.to_string(),
            blob_sha256: sha256.clone(),
            format: ArtifactFormat::TarZst,
            size_bytes,
            stored_at_unix: now_unix(),
            meta,
        };
        self.write_index(kind, input_key, &entry)?;

        Ok(sha256)
    }

    /// Restore the kernel payload (vmlinuz + modules) into `staging_dir` without
    /// deleting unrelated staging contents.
    pub fn restore_kernel_payload(&self, input_key: &str, staging_dir: &Path) -> Result<()> {
        let kind = "kernel_payload";
        let stored = self
            .get(kind, input_key)?
            .with_context(|| format!("No stored artifact for {kind}:{input_key}"))?;

        if stored.entry.format != ArtifactFormat::TarZst {
            bail!(
                "kernel_payload has unexpected format {:?} (expected tar_zst)",
                stored.entry.format
            );
        }

        // Verify blob hash on read.
        let (actual_sha, _sz) = sha256_file(&stored.blob_path)?;
        if actual_sha != stored.entry.blob_sha256 {
            bail!(
                "Blob hash mismatch for {}:{}\n  expected: {}\n  actual:   {}",
                kind,
                input_key,
                stored.entry.blob_sha256,
                actual_sha
            );
        }

        fs::create_dir_all(staging_dir)?;

        // Clean conflicting targets before extracting.
        let vmlinuz = staging_dir.join("boot/vmlinuz");
        if vmlinuz.exists() {
            let _ = fs::remove_file(&vmlinuz);
        }

        for dir in [
            staging_dir.join("lib/modules"),
            staging_dir.join("usr/lib/modules"),
        ] {
            if dir.exists() {
                let _ = fs::remove_dir_all(&dir);
            }
        }

        let f = File::open(&stored.blob_path)?;
        let decoder = zstd::stream::Decoder::new(f)?;
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(staging_dir)
            .with_context(|| format!("Failed to unpack {}", stored.blob_path.display()))?;

        Ok(())
    }

    /// Materialize an artifact from the store into the requested destination.
    ///
    /// - `ArtifactFormat::File`: `dest` is a file path.
    /// - `ArtifactFormat::TarZst`: `dest` is a directory path.
    pub fn materialize_to(&self, kind: &str, input_key: &str, dest: &Path) -> Result<()> {
        let stored = self
            .get(kind, input_key)?
            .with_context(|| format!("No stored artifact for {kind}:{input_key}"))?;

        if !stored.blob_path.exists() {
            bail!(
                "Blob missing for index entry {}:{} (expected {})",
                kind,
                input_key,
                stored.blob_path.display()
            );
        }

        // Verify blob hash on read (corruption detection).
        let (actual_sha, _sz) = sha256_file(&stored.blob_path)?;
        if actual_sha != stored.entry.blob_sha256 {
            bail!(
                "Blob hash mismatch for {}:{}\n  expected: {}\n  actual:   {}",
                kind,
                input_key,
                stored.entry.blob_sha256,
                actual_sha
            );
        }

        match stored.entry.format {
            ArtifactFormat::File => materialize_file(&stored.blob_path, dest),
            ArtifactFormat::TarZst => materialize_tar_zst_dir(&stored.blob_path, dest),
        }
    }

    /// List index entries for a kind.
    pub fn list_kind(&self, kind: &str) -> Result<Vec<IndexEntry>> {
        let dir = self.kind_dir(kind)?;
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut out = vec![];
        for ent in
            fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))?
        {
            let ent = ent?;
            let path = ent.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&path)?;
            let entry: IndexEntry = serde_json::from_slice(&bytes)
                .with_context(|| format!("Failed to parse index {}", path.display()))?;
            out.push(entry);
        }

        // Stable order.
        out.sort_by(|a, b| b.stored_at_unix.cmp(&a.stored_at_unix));
        Ok(out)
    }

    /// Best-effort garbage collection: remove blobs not referenced by any index entry.
    pub fn gc(&self) -> Result<usize> {
        let referenced = self.collect_referenced_blobs()?;

        let blobs_root = self.blobs_dir().join("sha256");
        if !blobs_root.exists() {
            return Ok(0);
        }

        let mut removed = 0usize;
        for ent in WalkDir::new(&blobs_root).into_iter().filter_map(Result::ok) {
            if !ent.file_type().is_file() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().to_string();
            if !is_hex_64(&name) {
                continue;
            }
            if referenced.contains(&name) {
                continue;
            }
            fs::remove_file(ent.path()).with_context(|| {
                format!(
                    "Failed to remove unreferenced blob {}",
                    ent.path().display()
                )
            })?;
            removed += 1;
        }

        Ok(removed)
    }

    /// Prune index entries, keeping only the newest `keep_last` per kind.
    /// Returns the number of index entries removed.
    pub fn prune_keep_last(&self, keep_last: usize) -> Result<usize> {
        if keep_last == 0 {
            bail!("keep_last must be >= 1");
        }

        let kinds = self.list_kinds()?;
        let mut removed = 0usize;

        for kind in kinds {
            let entries = self.list_kind(&kind)?;
            let mut to_remove = vec![];
            for (i, e) in entries.iter().enumerate() {
                if i >= keep_last {
                    to_remove.push(e.input_key.clone());
                }
            }

            for k in to_remove {
                let path = self.index_path(&kind, &k)?;
                if path.exists() {
                    fs::remove_file(&path)?;
                    removed += 1;
                }
            }
        }

        Ok(removed)
    }

    /// Return basic store statistics.
    pub fn status(&self) -> Result<StoreStatus> {
        let referenced = self.collect_referenced_blobs()?;
        let mut blob_bytes = 0u64;
        let mut blob_files = 0u64;
        for sha in &referenced {
            let p = self.blob_path(sha)?;
            if let Ok(md) = fs::metadata(&p) {
                blob_files += 1;
                blob_bytes += md.len();
            }
        }

        let mut index_files = 0u64;
        let idx = self.index_dir();
        if idx.exists() {
            for ent in WalkDir::new(&idx).into_iter().filter_map(Result::ok) {
                if ent.file_type().is_file()
                    && ent.path().extension().and_then(|s| s.to_str()) == Some("json")
                {
                    index_files += 1;
                }
            }
        }

        Ok(StoreStatus {
            root: self.root.clone(),
            index_entries: index_files,
            referenced_blobs: blob_files,
            referenced_bytes: blob_bytes,
        })
    }

    fn write_index(&self, kind: &str, input_key: &str, entry: &IndexEntry) -> Result<()> {
        let dir = self.kind_dir(kind)?;
        fs::create_dir_all(&dir)?;
        let path = self.index_path(kind, input_key)?;

        let bytes = serde_json::to_vec_pretty(entry)?;
        let tmp = self.tmp_dir().join(tmp_name("index.json"));
        fs::write(&tmp, bytes)?;
        atomic_rename(&tmp, &path)?;
        Ok(())
    }

    fn collect_referenced_blobs(&self) -> Result<BTreeSet<String>> {
        let idx = self.index_dir();
        let mut out = BTreeSet::new();
        if !idx.exists() {
            return Ok(out);
        }

        for ent in WalkDir::new(&idx).into_iter().filter_map(Result::ok) {
            if !ent.file_type().is_file() {
                continue;
            }
            if ent.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = match fs::read(ent.path()) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let entry: IndexEntry = match serde_json::from_slice(&bytes) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if is_hex_64(&entry.blob_sha256) {
                out.insert(entry.blob_sha256);
            }
        }
        Ok(out)
    }

    fn list_kinds(&self) -> Result<Vec<String>> {
        let idx = self.index_dir();
        if !idx.exists() {
            return Ok(vec![]);
        }
        let mut out = vec![];
        for ent in fs::read_dir(&idx)? {
            let ent = ent?;
            if ent.file_type()?.is_dir() {
                if let Some(s) = ent.file_name().to_str() {
                    out.push(s.to_string());
                }
            }
        }
        out.sort();
        Ok(out)
    }

    fn acquire_lock(&self, kind: &str, input_key: &str) -> Result<ArtifactLock> {
        let lock_path = self.lock_path(kind, input_key)?;
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Do not unlink "stale" lock files. Unlinking a still-locked file can
        // allow a second process to create a new lock file at the same path and
        // acquire a separate exclusive lock, defeating mutual exclusion.
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("Failed to create lock file: {}", lock_path.display()))?;

        if lock_file.try_lock_exclusive().is_err() {
            drop(lock_file);
            return Err(anyhow::anyhow!(
                "Artifact store key is locked by another process: {}",
                lock_path.display()
            ));
        }

        Ok(ArtifactLock {
            _file: lock_file,
            path: lock_path,
        })
    }
}

/// Centralized output directory for a distro crate directory.
///
/// In the superrepo, artifacts are written under:
/// `<repo>/.artifacts/out/<distro_dir_name>/`
///
/// Example:
/// - `.../LevitateOS/leviso` -> `.../LevitateOS/.artifacts/out/levitate`
pub fn central_output_dir_for_distro(base_dir: &Path) -> PathBuf {
    // In the monorepo, distro crates live at `<repo>/<DistroDir>`.
    // Use the parent as the repo root (standalone support can be added later).
    let repo_root = base_dir.parent().unwrap_or(base_dir);
    let distro_name = base_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("distro");
    let output_name = match distro_name {
        "leviso" => "levitate",
        "AcornOS" => "acorn",
        "IuppiterOS" => "iuppiter",
        "RalphOS" => "ralph",
        other => other,
    };
    repo_root
        .join(DEFAULT_STORE_DIR)
        .join(DEFAULT_OUTPUT_SUBDIR)
        .join(output_name)
}

/// Read an input key file (typically `output/.<artifact>-inputs.hash`) as a trimmed string.
/// Returns `Ok(None)` if the file doesn't exist or is empty.
pub fn read_input_key_file(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(path)
        .with_context(|| format!("Failed to read input key file {}", path.display()))?;
    let k = s.trim().to_string();
    if k.is_empty() {
        return Ok(None);
    }
    Ok(Some(k))
}

/// Best-effort restore for file artifacts.
///
/// Returns `Ok(true)` when the artifact was restored into `dest`.
pub fn try_restore_file_from_key(
    store: &ArtifactStore,
    kind: &str,
    key_file: &Path,
    dest: &Path,
) -> Result<bool> {
    if dest.exists() {
        return Ok(false);
    }
    let Some(key) = read_input_key_file(key_file)? else {
        return Ok(false);
    };
    if store.get(kind, &key)?.is_none() {
        return Ok(false);
    }
    store.materialize_to(kind, &key, dest)?;
    Ok(true)
}

/// Best-effort store for file artifacts.
///
/// Returns `Ok(Some(sha256))` when stored, `Ok(None)` when key file missing/empty.
pub fn try_store_file_from_key(
    store: &ArtifactStore,
    kind: &str,
    key_file: &Path,
    src_file: &Path,
    meta: BTreeMap<String, serde_json::Value>,
) -> Result<Option<String>> {
    let Some(key) = read_input_key_file(key_file)? else {
        return Ok(None);
    };
    let sha = store.put_blob_file(kind, &key, src_file, meta)?;
    Ok(Some(sha))
}

/// Best-effort restore for the kernel payload (vmlinuz + modules).
///
/// Returns `Ok(true)` when the payload was restored into `staging_dir`.
pub fn try_restore_kernel_payload_from_key(
    store: &ArtifactStore,
    key_file: &Path,
    staging_dir: &Path,
) -> Result<bool> {
    let vmlinuz = staging_dir.join("boot/vmlinuz");
    if vmlinuz.exists() {
        return Ok(false);
    }
    let Some(key) = read_input_key_file(key_file)? else {
        return Ok(false);
    };
    if store.get("kernel_payload", &key)?.is_none() {
        return Ok(false);
    }
    store.restore_kernel_payload(&key, staging_dir)?;
    Ok(true)
}

/// Best-effort store for the kernel payload (vmlinuz + modules).
///
/// Returns `Ok(Some(sha256))` when stored, `Ok(None)` when key file missing/empty.
pub fn try_store_kernel_payload_from_key(
    store: &ArtifactStore,
    key_file: &Path,
    staging_dir: &Path,
    meta: BTreeMap<String, serde_json::Value>,
) -> Result<Option<String>> {
    let Some(key) = read_input_key_file(key_file)? else {
        return Ok(None);
    };
    let sha = store.put_kernel_payload(&key, staging_dir, meta)?;
    Ok(Some(sha))
}

/// Basic store status.
#[derive(Debug, Clone)]
pub struct StoreStatus {
    pub root: PathBuf,
    pub index_entries: u64,
    pub referenced_blobs: u64,
    pub referenced_bytes: u64,
}

/// RAII guard: unlocks and removes the lock file on drop.
#[derive(Debug)]
struct ArtifactLock {
    #[allow(dead_code)]
    _file: File,
    path: PathBuf,
}

impl Drop for ArtifactLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn tmp_name(prefix: &str) -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{prefix}-{n}")
}

fn atomic_rename(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    // Prefer rename; within the store it's the same filesystem.
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_e) => {
            // Fall back to copy+remove (e.g. EXDEV).
            fs::copy(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            fs::remove_file(src)
                .with_context(|| format!("Failed to remove tmp {}", src.display()))?;
            Ok(())
        }
    }
}

fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let f = File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;
    let mut r = BufReader::new(f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 1024 * 1024];
    let mut size = 0u64;
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        size += n as u64;
    }
    let sha = format!("{:x}", hasher.finalize());
    Ok((sha, size))
}

fn validate_kind(kind: &str) -> Result<()> {
    if kind.is_empty() {
        bail!("artifact kind must not be empty");
    }
    if kind.contains('/') || kind.contains('\\') {
        bail!("artifact kind must not contain path separators: {kind}");
    }
    if kind.contains("..") {
        bail!("artifact kind must not contain '..': {kind}");
    }
    Ok(())
}

fn validate_key(key: &str) -> Result<()> {
    if key.is_empty() {
        bail!("artifact input key must not be empty");
    }
    if key.contains('/') || key.contains('\\') || key.contains("..") {
        bail!("artifact input key must be a safe filename segment");
    }
    Ok(())
}

fn validate_sha256(sha256: &str) -> Result<()> {
    if !is_hex_64(sha256) {
        bail!("invalid sha256: {sha256}");
    }
    Ok(())
}

fn is_hex_64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn materialize_file(blob: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    if dest.exists() {
        fs::remove_file(dest)
            .with_context(|| format!("Failed to remove existing {}", dest.display()))?;
    }

    // Fast path: hardlink (same filesystem, no copy cost).
    if fs::hard_link(blob, dest).is_ok() {
        return Ok(());
    }

    let tmp = dest.with_extension("tmp");
    fs::copy(blob, &tmp).with_context(|| {
        format!(
            "Failed to copy blob {} to {}",
            blob.display(),
            tmp.display()
        )
    })?;
    atomic_rename(&tmp, dest)?;
    Ok(())
}

fn hardlink_or_copy(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    if dest.exists() {
        let _ = fs::remove_file(dest);
    }

    if fs::hard_link(src, dest).is_ok() {
        return Ok(());
    }

    fs::copy(src, dest)
        .with_context(|| format!("Failed to copy {} to {}", src.display(), dest.display()))?;
    Ok(())
}

fn materialize_tar_zst_dir(blob: &Path, dest_dir: &Path) -> Result<()> {
    if dest_dir.exists() {
        fs::remove_dir_all(dest_dir)
            .with_context(|| format!("Failed to remove {}", dest_dir.display()))?;
    }

    let parent = dest_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let tmp = parent.join(tmp_name("extract"));
    fs::create_dir_all(&tmp)?;

    // Extract
    let f = File::open(blob)?;
    let decoder = zstd::stream::Decoder::new(f)?;
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(&tmp)
        .with_context(|| format!("Failed to unpack {}", blob.display()))?;

    // Atomic-ish: rename into place.
    // (Not fully atomic across filesystems, but tmp and dest are in same parent.)
    if dest_dir.exists() {
        fs::remove_dir_all(dest_dir)?;
    }
    fs::rename(&tmp, dest_dir).with_context(|| {
        format!(
            "Failed to move extracted dir {} to {}",
            tmp.display(),
            dest_dir.display()
        )
    })?;

    Ok(())
}

fn create_tar_zst(src_dir: &Path, out_path: &Path) -> Result<()> {
    let out = File::create(out_path)
        .with_context(|| format!("Failed to create {}", out_path.display()))?;
    let encoder = zstd::stream::Encoder::new(out, 3)?;
    let mut builder = TarBuilder::new(encoder);

    // Collect paths deterministically.
    let mut entries: Vec<PathBuf> = vec![];
    for ent in WalkDir::new(src_dir)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let p = ent.path();
        if p == src_dir {
            continue;
        }
        entries.push(p.to_path_buf());
    }

    entries.sort_by(|a, b| {
        let ra = a.strip_prefix(src_dir).unwrap_or(a).to_string_lossy();
        let rb = b.strip_prefix(src_dir).unwrap_or(b).to_string_lossy();
        ra.cmp(&rb)
    });

    for p in entries {
        let rel = p
            .strip_prefix(src_dir)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");

        let md = fs::symlink_metadata(&p)?;
        if md.is_dir() {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                header.set_mode(md.permissions().mode());
            }
            #[cfg(not(unix))]
            {
                header.set_mode(0o755);
            }
            header.set_cksum();
            builder.append_data(&mut header, rel, std::io::empty())?;
            continue;
        }

        if md.file_type().is_symlink() {
            let target = fs::read_link(&p)?;
            let target_str = target.to_string_lossy();
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                header.set_mode(md.permissions().mode());
            }
            #[cfg(not(unix))]
            {
                header.set_mode(0o777);
            }
            header.set_link_name(target_str.as_ref())?;
            header.set_cksum();
            builder.append_data(&mut header, rel, std::io::empty())?;
            continue;
        }

        if md.is_file() {
            let mut f = File::open(&p)?;
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Regular);
            header.set_size(md.len());
            header.set_mtime(0);
            header.set_uid(0);
            header.set_gid(0);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                header.set_mode(md.permissions().mode());
            }
            #[cfg(not(unix))]
            {
                header.set_mode(0o644);
            }
            header.set_cksum();
            builder.append_data(&mut header, rel, &mut f)?;
            continue;
        }
    }

    let encoder = builder
        .into_inner()
        .with_context(|| "Failed to finalize tar builder")?;
    encoder.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn file_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let store = ArtifactStore::open(&repo).unwrap();
        let kind = "rootfs_erofs";
        let key = "deadbeef";

        let src = tmp.path().join("src.bin");
        fs::write(&src, b"hello").unwrap();

        let sha = store
            .put_blob_file(kind, key, &src, BTreeMap::new())
            .unwrap();
        assert!(is_hex_64(&sha));

        let dest = tmp.path().join("out.bin");
        store.materialize_to(kind, key, &dest).unwrap();
        let out = fs::read(&dest).unwrap();
        assert_eq!(out, b"hello");
    }

    #[test]
    fn dir_tar_zst_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();

        let store = ArtifactStore::open(&repo).unwrap();
        let kind = "kernel_payload";
        let key = "cafebabe";

        let src_dir = tmp.path().join("staging");
        fs::create_dir_all(src_dir.join("boot")).unwrap();
        fs::write(src_dir.join("boot/vmlinuz"), b"kernel").unwrap();

        store
            .put_dir_as_tar_zst(kind, key, &src_dir, BTreeMap::new())
            .unwrap();

        let dest_dir = tmp.path().join("out-staging");
        store.materialize_to(kind, key, &dest_dir).unwrap();
        let bytes = fs::read(dest_dir.join("boot/vmlinuz")).unwrap();
        assert_eq!(bytes, b"kernel");
    }
}
