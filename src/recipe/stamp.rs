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
//! Never used in exact mode (a release reuses only what byte-identical tools
//! made), for forced runs (`--force-acquire`, `--force-build`, `--autofix`) or
//! for phases other than `install` (a check can depend on state its outputs do
//! not show).

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
const FORMAT: &str = "1";
const DIRECTORY: &str = ".builder-stamps";
const SMALL_FILE: u64 = 1 << 20;
const FORCING: &[&str] = &["--force-acquire", "--force-build", "--autofix"];

pub(crate) struct Stamp {
    path: PathBuf,
    key: String,
}

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
        if phase != "install" || super::exact_identity() {
            return Ok(None);
        }
        let mut hash = Sha256::new();
        let mut field = |name: &str, value: &str| {
            for text in [name, value] {
                hash.update((text.len() as u64).to_le_bytes());
                hash.update(text.as_bytes());
            }
        };
        field("format", FORMAT);
        field("phase", phase);
        field("recipe", &recipe.display().to_string());
        field("recipe-closure", &crate::compose::recipe_identity(repo, recipe)?);
        field("work", &work.display().to_string());
        field("root", root);
        field("behaviour", &crate::prebuilt::behaviour());
        if let Some(worker) = worker {
            field("worker", &tree_digest(worker)?);
        }
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            let arg = arg.to_string_lossy();
            if FORCING.contains(&arg.as_ref()) {
                return Ok(None);
            }
            if arg == "--input" {
                let value = args.next().context("--input without a value")?.to_string_lossy();
                let (name, path) = value.split_once('=').context("--input without NAME=PATH")?;
                field("input", name);
                field("input-content", &tree_digest(Path::new(path))?);
            } else {
                field("arg", &arg);
            }
        }
        let key: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        Ok(Some(Self {
            path: work.join(DIRECTORY).join(format!("{key}.json")),
            key,
        }))
    }

    pub(crate) fn short(&self) -> &str {
        &self.key[..12]
    }

    /// The stamped result, when the stamp exists and every output it names is
    /// still exactly what the run published. Anything else is `None`: the
    /// caller runs the recipe.
    pub(crate) fn reuse(&self) -> Option<RecipeResult> {
        let stamp: Value = serde_json::from_slice(&fs::read(&self.path).ok()?).ok()?;
        if stamp.get("key")?.as_str()? != self.key {
            return None;
        }
        let ctx = stamp.get("result")?;
        let recorded = stamp.get("outputs")?.as_str()?;
        if outputs_digest(ctx).ok()? != recorded {
            return None;
        }
        RecipeResult::from_json(&serde_json::to_vec(ctx).ok()?).ok()
    }

    /// Records a successful run's result and the digest of its outputs.
    pub(crate) fn save(&self, result: &RecipeResult) -> Result<()> {
        let ctx = result.ctx();
        let record = serde_json::json!({
            "format": FORMAT,
            "key": self.key,
            "outputs": outputs_digest(ctx)?,
            "result": ctx,
            "saved_at": distro_contract::runs::now_rfc3339(),
        });
        // Write and rename, without fsync: a stamp lost in a crash only means
        // the step runs again.
        let parent = self.path.parent().context("a stamp has a directory")?;
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        let temporary = parent.join(format!(".{}.tmp-{}", self.short(), std::process::id()));
        fs::write(&temporary, serde_json::to_vec_pretty(&record)?)
            .and_then(|_| fs::rename(&temporary, &self.path))
            .with_context(|| format!("writing the stamp {}", self.path.display()))
    }
}

/// A digest of every output path a result names (`ctx.outputs`), in name order.
fn outputs_digest(ctx: &Value) -> Result<String> {
    let mut hash = Sha256::new();
    if let Some(outputs) = ctx.get("outputs").and_then(Value::as_object) {
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
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

/// What a file or tree is, as the module comment describes.
fn tree_digest(root: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    for item in walkdir::WalkDir::new(root).follow_links(false).sort_by_file_name() {
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
                hash.update(format!("{} {} {} {}", md.len(), md.mtime_nsec() + md.mtime() * 1_000_000_000, md.dev(), md.ino()).as_bytes());
            }
        }
        hash.update([0]);
    }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}
