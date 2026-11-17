//! Done stamps: a Recipe `install` the builder already ran, with the same
//! recipe, inputs, defines, root and Recipe behaviour, and whose outputs are
//! still exactly what it published, is not run again; the builder reuses its
//! result. Each run otherwise starts a Ginux domain (unpacking the compiler
//! image into a fresh root) just to find nothing to do: minutes per step under
//! load, and a desktop build has about fifteen such steps before the first one
//! that works. A retry after a failure redoes only what failed and after.
//!
//! The stamp lives at `<work>/.builder-stamps/<key>.json` and holds the run's
//! result and a digest of its outputs. Its key is a SHA-256 over the phase,
//! the recipe's source closure (`compose::recipe_identity`), the work
//! directory, every flag with each `--input` replaced by a digest of what it
//! names, the root the run uses, the worker override, and Recipe's and
//! Ginux's behaviour versions (`prebuilt::behaviour`).
//!
//! An input or output tree is described by every entry's path, kind and mode,
//! a symlink's target, a small file's (≤ 1 MiB) content digest and a large
//! file's size, modification time and inode: a regenerated text input keeps
//! its digest, and a multi-GB tree costs a `stat` per file, not a re-read.
//!
//! Never reused in exact mode (a release reuses only what byte-identical
//! tools made), and never made for redone runs (`Invocation::redo`, which
//! cleans the program up first) or for phases other than `install`.
//!
//! Each stamp names its slot: the phase, the recipe, the work directory and
//! the flags with each `--input` by its name only, so a run whose inputs or
//! recipe changed is in the same slot as the run it replaces. A caller whose
//! results are only read while the builder runs (`Invocation::discard_superseded`)
//! keeps one result per slot: after its run the other stamps of the slot, and
//! the publications they name, are discarded through Recipe
//! (`levitate_recipe::state::discard_publications`); that call also makes the
//! exact build's stamp, never reused, so its publication is discarded by the
//! next run of the slot. A process holds a shared `flock` on every stamp whose
//! result it got, until it exits; a stamp is discarded only under an
//! exclusive one taken without waiting, so no result a running builder got
//! is ever discarded.

use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::RecipeResult;

/// Bump when the key or the stamp's meaning changes in a way no field shows.
const FORMAT: &str = "2";
const DIRECTORY: &str = ".builder-stamps";
const SMALL_FILE: u64 = 1 << 20;

pub(crate) struct Stamp {
    path: PathBuf,
    key: String,
    slot: String,
}

/// The stamps whose results this process got, each locked shared until it
/// exits.
static HELD: std::sync::Mutex<Vec<fs::File>> = std::sync::Mutex::new(Vec::new());

impl Stamp {
    /// The stamp of a run, or `None` when stamps never apply to it.
    pub(crate) fn for_run(
        repo: &Path,
        phase: &str,
        recipe: &Path,
        work: &Path,
        args: &[OsString],
        root: &str,
        worker: Option<&Path>,
    ) -> Result<Option<Self>> {
        if phase != "install" {
            return Ok(None);
        }
        let mut hash = Sha256::new();
        let mut slot = Sha256::new();
        let mut field = |name: &str, value: &str, in_slot: bool| {
            for text in [name, value] {
                hash.update((text.len() as u64).to_le_bytes());
                hash.update(text.as_bytes());
                if in_slot {
                    slot.update((text.len() as u64).to_le_bytes());
                    slot.update(text.as_bytes());
                }
            }
        };
        field("format", FORMAT, true);
        field("phase", phase, true);
        field("recipe", &recipe.display().to_string(), true);
        field(
            "recipe-closure",
            &crate::compose::recipe_identity(repo, recipe)?,
            false,
        );
        field("work", &work.display().to_string(), true);
        field("root", root, false);
        field("behaviour", &crate::prebuilt::behaviour(), false);
        if let Some(worker) = worker {
            field("worker", &tree_digest(worker)?, false);
        }
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let arg = arg.to_string_lossy();
            if arg == "--input" {
                let value = args
                    .next()
                    .context("--input without a value")?
                    .to_string_lossy();
                let (name, path) = value.split_once('=').context("--input without NAME=PATH")?;
                field("input", name, true);
                field("input-content", &tree_digest(Path::new(path))?, false);
            } else {
                field("arg", &arg, true);
            }
        }
        let hex = |digest: &[u8]| -> String { digest.iter().map(|byte| format!("{byte:02x}")).collect() };
        let key = hex(&hash.finalize());
        Ok(Some(Self {
            path: work.join(DIRECTORY).join(format!("{key}.json")),
            key,
            slot: hex(&slot.finalize()),
        }))
    }

    pub(crate) fn short(&self) -> &str {
        &self.key[..12]
    }

    /// The stamped result, when the stamp exists and every output it names is
    /// still exactly what the run published. Anything else is `None`: the
    /// caller runs the recipe.
    /// The stamp is locked shared first and read through that lock, which
    /// this process then holds until it exits.
    pub(crate) fn reuse(&self) -> Option<RecipeResult> {
        let mut file = fs::File::open(&self.path).ok()?;
        flock(&file, libc::LOCK_SH).ok()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).ok()?;
        let stamp: Value = serde_json::from_slice(&bytes).ok()?;
        if stamp.get("key")?.as_str()? != self.key {
            return None;
        }
        let result = stamp.get("result")?;
        let recorded = stamp.get("outputs")?.as_str()?;
        if outputs_digest(result).ok()? != recorded {
            return None;
        }
        let result = RecipeResult::from_json(&serde_json::to_vec(result).ok()?).ok()?;
        hold(file);
        Some(result)
    }

    /// Records a successful run's result and the digest of its outputs.
    /// The stamp is locked shared before it is renamed into place, and this
    /// process holds that lock until it exits.
    pub(crate) fn save(&self, result: &RecipeResult) -> Result<()> {
        let json = result.json();
        let record = serde_json::json!({
            "format": FORMAT,
            "key": self.key,
            "slot": self.slot,
            "outputs": outputs_digest(json)?,
            "result": json,
            "saved_at": distro_contract::runs::now_rfc3339(),
        });
        // Write and rename, without fsync: a stamp lost in a crash only means
        // the step runs again.
        let parent = self.path.parent().context("a stamp has a directory")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let temporary = parent.join(format!(".{}.tmp-{}", self.short(), std::process::id()));
        let written = (|| -> std::io::Result<fs::File> {
            fs::write(&temporary, serde_json::to_vec_pretty(&record)?)?;
            let file = fs::File::open(&temporary)?;
            flock(&file, libc::LOCK_SH)?;
            fs::rename(&temporary, &self.path)?;
            Ok(file)
        })();
        match written {
            Ok(file) => {
                hold(file);
                Ok(())
            }
            Err(error) => {
                let _ = fs::remove_file(&temporary);
                Err(error).with_context(|| format!("writing the stamp {}", self.path.display()))
            }
        }
    }

    /// The outputs the stamp file of this key records now, read under an
    /// exclusive lock taken without waiting, which the returned file keeps
    /// until it drops; `None` when there is no such stamp or someone holds it.
    /// Call it before [`Self::save`] replaces that file, and pass the result
    /// to [`Self::discard_superseded`].
    pub(crate) fn previous(&self) -> Option<(fs::File, Value)> {
        let (file, record) = lock_record(&self.path)?;
        Some((file, record.get("result")?.get("outputs")?.clone()))
    }

    /// After this stamp was saved or reused for `result`: discards, through
    /// Recipe, the publications the other stamps of its slot name (and
    /// `previous`, what this key's stamp named before it was replaced), and
    /// removes those stamps. A stamp some process holds is left, and so is
    /// everything while Recipe reports the publications of `state` (the
    /// run's state directory) in use. Returns how many results were
    /// discarded.
    pub(crate) fn discard_superseded(
        &self,
        state: &Path,
        result: &RecipeResult,
        previous: Option<(fs::File, Value)>,
    ) -> Result<usize> {
        let dir = self.path.parent().context("a stamp has a directory")?;
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error).with_context(|| format!("listing {}", dir.display())),
        };
        // The publications this run's result lives in are never discarded.
        let kept: Vec<PathBuf> = publications(result.json().get("outputs").unwrap_or(&Value::Null));
        let shares = |outputs: &Value| publications(outputs).iter().any(|dir| kept.contains(dir));
        let mut stale = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
            if path == self.path || name.starts_with('.') || !name.ends_with(".json") {
                continue;
            }
            let Some((file, record)) = lock_record(&path) else {
                continue;
            };
            if record.get("slot").and_then(Value::as_str) != Some(self.slot.as_str()) {
                continue;
            }
            let outputs = record
                .get("result")
                .and_then(|result| result.get("outputs"))
                .cloned()
                .unwrap_or(Value::Null);
            if !shares(&outputs) {
                stale.push((path, file, outputs));
            }
        }
        let previous = previous.filter(|(_, outputs)| !shares(outputs));
        let results: Vec<&Value> = stale
            .iter()
            .map(|(_, _, outputs)| outputs)
            .chain(previous.as_ref().map(|(_, outputs)| outputs))
            .collect();
        if results.is_empty() {
            return Ok(0);
        }
        if !levitate_recipe::state::discard_publications(Some(state), &results)? {
            crate::tprintln!(
                "  (the superseded results in {} are kept for now: a run holds its publications)",
                state.display()
            );
            return Ok(0);
        }
        for (path, file, _) in &stale {
            // Only the file locked here: one renamed into its place since stays.
            let same = fs::symlink_metadata(path).ok().zip(file.metadata().ok());
            if same.is_some_and(|(at, locked)| at.dev() == locked.dev() && at.ino() == locked.ino()) {
                let _ = fs::remove_file(path);
            }
        }
        Ok(results.len())
    }
}

/// The stamp file at `path` and its record, locked exclusively without
/// waiting and read through that lock; `None` when it is absent, held or
/// unreadable.
fn lock_record(path: &Path) -> Option<(fs::File, Value)> {
    let mut file = fs::File::open(path).ok()?;
    flock(&file, libc::LOCK_EX | libc::LOCK_NB).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let record = serde_json::from_slice(&bytes).ok()?;
    Some((file, record))
}

/// The directories the output paths of `outputs` (a result's `outputs`) are in.
fn publications(outputs: &Value) -> Vec<PathBuf> {
    outputs
        .as_object()
        .into_iter()
        .flat_map(|outputs| outputs.values())
        .filter_map(Value::as_str)
        .filter_map(|path| Path::new(path).parent().map(Path::to_path_buf))
        .collect()
}

/// Keeps `file` (and its lock) open until the process exits.
fn hold(file: fs::File) {
    HELD.lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(file);
}

fn flock(file: &fs::File, operation: libc::c_int) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    loop {
        // SAFETY: flock only reads the descriptor of the open file.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// A digest of every output path a result names (`outputs`), in name order.
fn outputs_digest(result: &Value) -> Result<String> {
    let mut hash = Sha256::new();
    if let Some(outputs) = result.get("outputs").and_then(Value::as_object) {
        let mut names: Vec<&String> = outputs.keys().collect();
        names.sort();
        for name in names {
            hash.update(name.as_bytes());
            hash.update([0]);
            match outputs[name].as_str() {
                Some(path) => hash.update(tree_digest(Path::new(path))?.as_bytes()),
                None => hash.update(outputs[name].to_string().as_bytes()),
            }
            hash.update([0]);
        }
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// What a file or tree is, as the module comment describes.
fn tree_digest(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    for item in walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
    {
        let item = item.with_context(|| format!("reading {}", root.display()))?;
        let md = item.metadata()?;
        let relative = item.path().strip_prefix(root).unwrap_or(item.path());
        hash.update(relative.as_os_str().as_encoded_bytes());
        hash.update([0]);
        hash.update(format!("{:o}", md.mode()).as_bytes());
        if md.file_type().is_symlink() {
            hash.update(fs::read_link(item.path())?.as_os_str().as_encoded_bytes());
        } else if md.is_file() {
            if md.len() <= SMALL_FILE {
                let mut file = fs::File::open(item.path())?;
                let mut bytes = Vec::with_capacity(md.len() as usize);
                file.read_to_end(&mut bytes)?;
                hash.update(Sha256::digest(&bytes));
            } else {
                hash.update(
                    format!(
                        "{} {} {} {}",
                        md.len(),
                        md.mtime_nsec() + md.mtime() * 1_000_000_000,
                        md.dev(),
                        md.ino()
                    )
                    .as_bytes(),
                );
            }
        }
        hash.update([0]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
