use std::cmp::Reverse;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const RUN_MANIFEST_FILENAME: &str = "run-manifest.json";

/// `status` of a finished, successful run.
pub const STATUS_SUCCESS: &str = "success";
/// `status` of a failed run.
pub const STATUS_FAILED: &str = "failed";
/// `status` of a run still in progress.
pub const STATUS_BUILDING: &str = "building";
/// `target_kind` of a release product build.
pub const TARGET_KIND_RELEASE_PRODUCT: &str = "release-product";

/// `<repo>/.artifacts/out/<distro>`: everything built for one variant.
pub fn distro_output_dir(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root.join(".artifacts").join("out").join(distro_id)
}

/// The runs of one release product, `<distro output>/releases/<product>`.
pub fn release_product_dir(
    repo_root: &Path,
    distro_id: &str,
    product: distro_contract::ProductId,
) -> PathBuf {
    distro_output_dir(repo_root, distro_id)
        .join("releases")
        .join(product.name())
}

/// The current kernel build of a variant.
pub fn kernel_output_dir(repo_root: &Path, distro_id: &str) -> PathBuf {
    repo_root
        .join(".artifacts")
        .join("kernel")
        .join(distro_id)
        .join("current")
}
const RUN_ID_SALT_BITS: u32 = 32;
static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize, Clone)]
pub struct RunMetadata {
    pub run_id: String,
    pub status: String,
    pub created_at_utc: String,
    pub finished_at_utc: Option<String>,
    #[serde(default)]
    pub target_kind: Option<String>,
    #[serde(default)]
    pub target_name: Option<String>,
    #[serde(default)]
    pub iso_path: Option<String>,
}

/// The run manifest in `run_dir`, if there is one.
pub fn read_run_manifest(run_dir: &Path) -> Result<Option<RunMetadata>> {
    let path = run_manifest_path(run_dir);
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("reading run metadata '{}'", path.display()))?;
    Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
        format!("parsing run metadata '{}'", path.display())
    })?))
}

pub fn run_manifest_path(run_dir: &Path) -> PathBuf {
    run_dir.join(RUN_MANIFEST_FILENAME)
}

pub fn load_run_metadata(run_root_dir: &Path) -> Result<Vec<RunMetadata>> {
    if !run_root_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(run_root_dir)
        .with_context(|| format!("reading run directory '{}'", run_root_dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("iterating run directory '{}'", run_root_dir.display()))?;
        let run_dir = entry.path();
        if !run_dir.is_dir() {
            continue;
        }
        let Some(run_name) = run_dir.file_name().and_then(|part| part.to_str()) else {
            continue;
        };
        if run_name.starts_with('.') {
            continue;
        }
        if let Some(parsed) = read_run_manifest(&run_dir)? {
            runs.push(parsed);
        }
    }
    Ok(runs)
}

pub fn latest_successful_run_id(run_root_dir: &Path) -> Result<Option<String>> {
    let mut runs = load_run_metadata(run_root_dir)?;
    runs.retain(|run| run.status == STATUS_SUCCESS);
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    Ok(runs.first().map(|r| r.run_id.clone()))
}

pub fn prune_old_runs(run_root_dir: &Path, keep: usize) -> Result<()> {
    let mut runs = load_run_metadata(run_root_dir)?;
    runs.sort_by_key(|run| Reverse(run_sort_key(run)));
    for run in runs.into_iter().skip(keep) {
        let path = run_root_dir.join(&run.run_id);
        fs::remove_dir_all(&path)
            .with_context(|| format!("removing expired run directory '{}'", path.display()))?;
    }
    Ok(())
}

pub fn allocate_run_dir(run_root_dir: &Path) -> Result<(String, PathBuf)> {
    fs::create_dir_all(run_root_dir).with_context(|| {
        format!(
            "creating run output root directory '{}'",
            run_root_dir.display()
        )
    })?;
    for _ in 0..32 {
        let run_id = generate_run_id()?;
        let run_root = run_root_dir.join(&run_id);
        if run_root.exists() {
            continue;
        }
        fs::create_dir_all(&run_root)
            .with_context(|| format!("creating run output directory '{}'", run_root.display()))?;
        return Ok((run_id, run_root));
    }
    bail!(
        "failed allocating unique run directory under '{}'",
        run_root_dir.display()
    )
}

fn run_sort_key(run: &RunMetadata) -> String {
    run.finished_at_utc
        .clone()
        .unwrap_or_else(|| run.created_at_utc.clone())
}

fn generate_run_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH while generating run id")?
        .as_nanos();
    let pid_component = (std::process::id() as u128) << (RUN_ID_SALT_BITS - 16);
    let seq_component = (RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed) as u128) & 0xFFFF;
    let entropy = (nanos << RUN_ID_SALT_BITS) | pid_component | seq_component;
    let mut suffix = base62_encode_u128(entropy);
    suffix = suffix.trim_start_matches('0').to_string();
    if suffix.is_empty() {
        suffix.push('0');
    }
    if suffix.len() > 20 {
        bail!("sortable run id overflow while generating run identifier")
    }
    Ok(suffix)
}

fn base62_encode_u128(mut value: u128) -> String {
    const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut bytes = Vec::new();
    while value > 0 {
        let idx = (value % 62) as usize;
        bytes.push(ALPHABET[idx] as char);
        value /= 62;
    }
    bytes.iter().rev().collect()
}
