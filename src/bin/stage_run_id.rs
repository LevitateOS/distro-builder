use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

const RUN_ID_SALT_BITS: u32 = 32;
static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn generate_stage_run_id() -> Result<String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before UNIX_EPOCH while generating stage run id")?
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
        bail!("sortable stage run id overflow while generating run identifier")
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
