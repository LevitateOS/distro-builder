//! The unpack store and composed system trees (`docs/build-redesign.md`,
//! sections 1 and 2).
//!
//! - **Unpack store**: `.artifacts/store/unpacked/<unpack key>/`, one locked
//!   RPM each: `root/` (what `target_extract_rpm` makes of the package in an
//!   empty root, its target metadata manifest included), `members.json` (the
//!   record of that extraction, `levitate_recipe::target::Members`) and
//!   `unpacked.json` (what the entry is). The key names what determines the
//!   tree: the package's SHA-256, the unpack recipe's sources, Recipe's
//!   declared helper behaviour and the root image the recipe runs in. Entries
//!   appear by an atomic rename of a finished temp directory and are never
//!   deleted by a build. Missing packages are unpacked by
//!   `fedora-rpm-unpack.rhai` inside Ginux, in shards that run at once.
//! - **Composed trees**: `.artifacts/store/composed/<compose key>/`: `root/`
//!   (the packages merged in lock order, the files hardlinked from the unpack
//!   store), `tree.tar` (`root/` in `target::write_tree`'s format, made when a
//!   recipe first needs it), `packages.txt`, `owners.jsonl` and
//!   `composed.json`. The key names the ordered unpack keys. Composition runs
//!   no package code and no Ginux: `target::replay` applies each package's
//!   recorded extraction to the tree exactly as extracting the packages one
//!   after another into one root does, the manifest included. The desktop
//!   tree is the base tree (its files linked, not composed again) with the
//!   desktop lock's packages replayed over it.
//!
//! Recipes take a composed tree as a `composed` directory input holding
//! `tree.tar` and `packages.txt` ([`input_view`]), and check `packages.txt`
//! against their locks.

use anyhow::{bail, ensure, Context, Result};
use levitate_recipe::target::{self, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::recipe::rootfs_source::LockRow;
use crate::recipe::Invocation;

/// The recipe that unpacks RPMs into the store.
pub(crate) const RPM_UNPACK_RECIPE: &str = "distro/builder/os-recipes/fedora-rpm-unpack.rhai";
/// The version of the unpack store's layout (`root/`, `members.json`,
/// `unpacked.json`); part of every unpack key.
pub(crate) const UNPACK_FORMAT: u32 = 1;
/// The version of composition and of the composed tree's layout; part of
/// every compose key.
pub(crate) const COMPOSE_FORMAT: u32 = 1;
/// At most this many package bytes (compressed) and packages go to one
/// unpack shard: a Ginux domain keeps its files in memory, and the unpack
/// recipe's object limit is sized for one shard.
const SHARD_BYTES: u64 = 512 * 1024 * 1024;
const SHARD_PACKAGES: usize = 250;
/// At most this many shards run at once.
const MAX_WORKERS: usize = 6;

pub(crate) fn store(repo: &Path) -> PathBuf {
    repo.join(".artifacts/store")
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A key over named fields, each length-prefixed.
pub(crate) fn key(fields: &[(&str, &str)]) -> String {
    let mut hasher = Sha256::new();
    for (name, value) in fields {
        for text in [name, value] {
            hasher.update((text.len() as u64).to_le_bytes());
            hasher.update(text.as_bytes());
        }
    }
    hex(&hasher.finalize())
}

/// A locked package and the file it is in.
#[derive(Debug, Clone)]
pub(crate) struct Package {
    pub row: LockRow,
    pub file: PathBuf,
}

/// A locked package's entry in the unpack store.
#[derive(Debug, Clone)]
pub(crate) struct Unpacked {
    pub row: LockRow,
    pub key: String,
    pub dir: PathBuf,
}

/// A composed tree in the store.
#[derive(Debug, Clone)]
pub(crate) struct Composed {
    pub key: String,
    pub dir: PathBuf,
    /// Its packages in the order they were composed.
    pub packages: Vec<LockRow>,
    pub unpack_keys: Vec<String>,
}

impl Composed {
    pub fn root(&self) -> PathBuf {
        self.dir.join("root")
    }
}

/// The file of each of `rows`: the download store's
/// (`.artifacts/store/sha256/<digest>`), else the one a preseed left in one
/// of `preseed_dirs` (under its URL's basename). Every file is verified
/// against its SHA-256 inside Ginux before it is unpacked.
pub(crate) fn package_files(repo: &Path, rows: &[LockRow], preseed_dirs: &[PathBuf]) -> Result<Vec<Package>> {
    let downloads = crate::recipe::rootfs_source::store_dir(repo);
    rows.iter()
        .map(|row| {
            let stored = downloads.join(&row.sha256);
            let file = if stored.is_file() {
                stored
            } else {
                preseed_dirs
                    .iter()
                    .map(|dir| dir.join(row.file_name()))
                    .find(|file| file.is_file())
                    .with_context(|| {
                        format!(
                            "the package {} ({}) is neither in the download store nor in a preseed output; \
                             run the rootfs source preseed",
                            row.id, row.sha256
                        )
                    })?
            };
            Ok(Package { row: row.clone(), file })
        })
        .collect()
}

/// What the unpack recipe's results depend on besides the package: its
/// sources (the recipe and its imports, by content), as one line.
pub(crate) fn recipe_identity(repo: &Path, recipe: &Path) -> Result<String> {
    let os_recipes = repo.join("distro/builder/os-recipes");
    let declaration = levitate_recipe::needs::declaration(recipe, Some(&os_recipes))
        .with_context(|| format!("reading the declaration of '{}'", recipe.display()))?;
    let base = os_recipes.canonicalize().unwrap_or(os_recipes);
    Ok(declaration
        .source_digests
        .iter()
        .map(|(path, digest)| {
            let name = path.strip_prefix(&base).unwrap_or(path);
            format!("{}={digest}", name.display())
        })
        .collect::<Vec<_>>()
        .join(";"))
}

/// The unpack key of the package with SHA-256 `sha256`.
fn unpack_key(sha256: &str, recipe: &str, root: &str) -> String {
    key(&[
        ("store", "unpacked"),
        ("format", &UNPACK_FORMAT.to_string()),
        // Switch to Recipe's helper behaviour version: it changes whenever a
        // helper's output can change, target_extract_rpm's included.
        ("recipe-engine", &levitate_recipe::identity::ENGINE_BEHAVIOUR.to_string()),
        ("recipe", recipe),
        ("root", root),
        ("package", sha256),
    ])
}

fn is_unpacked(dir: &Path) -> bool {
    dir.join("unpacked.json").is_file() && dir.join("members.json").is_file() && dir.join("root").is_dir()
}

/// `unpacked.json`: what an unpack store entry is.
#[derive(Serialize)]
struct UnpackedRecord<'a> {
    format: u32,
    key: &'a str,
    name: &'a str,
    id: &'a str,
    sha256: &'a str,
    recipe: &'a str,
    root: &'a str,
    recipe_engine: u32,
    unpacked_at: String,
}

/// The unpack store entries of `packages`, in their order: the ones the store
/// lacks are unpacked first, in shards that run at once.
pub(crate) fn unpack(repo: &Path, packages: &[Package]) -> Result<Vec<Unpacked>> {
    let recipe = repo.join(RPM_UNPACK_RECIPE);
    let identity = recipe_identity(repo, &recipe)?;
    let root = crate::recipe::root_reference(repo)?;
    let dir = store(repo).join("unpacked");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let entries: Vec<Unpacked> = packages
        .iter()
        .map(|package| {
            let key = unpack_key(&package.row.sha256, &identity, &root);
            Unpacked { row: package.row.clone(), dir: dir.join(&key), key }
        })
        .collect();
    let missing = |entries: &[Unpacked]| -> Vec<usize> {
        let mut seen = BTreeSet::new();
        (0..entries.len())
            .filter(|&i| !is_unpacked(&entries[i].dir) && seen.insert(entries[i].key.clone()))
            .collect()
    };
    let wanted = missing(&entries);
    if wanted.is_empty() {
        crate::tprintln!("  unpack store: all {} packages unpacked already", entries.len());
        return Ok(entries);
    }
    // Another build may be unpacking some of the same packages: wait for it,
    // then unpack only what is still missing.
    let _locks = lock_buckets(&dir, wanted.iter().map(|&i| entries[i].key.as_str()), "unpacking")?;
    let wanted = missing(&entries);
    if !wanted.is_empty() {
        let jobs: Vec<Job> = wanted
            .iter()
            .map(|&i| Job {
                row: entries[i].row.clone(),
                key: entries[i].key.clone(),
                file: packages[i].file.clone(),
                bytes: fs::metadata(&packages[i].file).map(|m| m.len()).unwrap_or(0),
            })
            .collect();
        crate::tprintln!(
            "  unpack store: {} of {} packages to unpack",
            jobs.len(),
            entries.len()
        );
        run_shards(repo, &recipe, &dir, &identity, &root, jobs)?;
    }
    if let Some(&i) = missing(&entries).first() {
        bail!("the unpack store still lacks {} after unpacking", entries[i].row.id);
    }
    Ok(entries)
}

/// One package to unpack.
#[derive(Debug, Clone)]
struct Job {
    row: LockRow,
    key: String,
    file: PathBuf,
    bytes: u64,
}

/// `jobs` split into shards of at most [`SHARD_BYTES`] and [`SHARD_PACKAGES`],
/// at least as many as workers run, balanced by bytes (largest first, each to
/// the lightest shard).
fn shards(mut jobs: Vec<Job>, workers: usize) -> Vec<Vec<Job>> {
    let bytes: u64 = jobs.iter().map(|job| job.bytes).sum();
    let count = workers
        .max(bytes.div_ceil(SHARD_BYTES) as usize)
        .max(jobs.len().div_ceil(SHARD_PACKAGES))
        .min(jobs.len())
        .max(1);
    jobs.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.key.cmp(&b.key)));
    let mut shards: Vec<(u64, Vec<Job>)> = vec![(0, Vec::new()); count];
    for job in jobs {
        let lightest = shards
            .iter_mut()
            .min_by_key(|(bytes, jobs)| (*bytes, jobs.len()))
            .expect("at least one shard");
        lightest.0 += job.bytes;
        lightest.1.push(job);
    }
    shards.into_iter().map(|(_, jobs)| jobs).filter(|jobs| !jobs.is_empty()).collect()
}

/// How many shards run at once: the CPUs but one, at least one, at most
/// [`MAX_WORKERS`].
fn workers() -> usize {
    let cpus = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    cpus.saturating_sub(1).clamp(1, MAX_WORKERS)
}

/// Unpacks `jobs` into the store `dir`: in shards, [`workers`] at a time,
/// each its own Recipe invocation with its own work directory, log and
/// progress file. Each finished shard's trees move into the store at once, so
/// a failed shard costs only its own packages.
fn run_shards(repo: &Path, recipe: &Path, dir: &Path, identity: &str, root: &str, jobs: Vec<Job>) -> Result<()> {
    let total = jobs.len();
    let workers = workers();
    let shards = shards(jobs, workers);
    let count = shards.len();
    let running = workers.min(count);
    // The launcher and Recipe are built once, before the threads need them.
    crate::recipe::binary(repo)?;
    crate::recipe::ginux_binary(repo)?;
    let work = repo.join(".artifacts/work/unpack");
    let logs = work.join("logs");
    fs::create_dir_all(&logs).with_context(|| format!("creating {}", logs.display()))?;
    remove_dead_shards(&work)?;
    let progress_dir = crate::timeline::progress_dir(repo).join("unpack");
    fs::create_dir_all(&progress_dir).with_context(|| format!("creating {}", progress_dir.display()))?;
    let pid = std::process::id();
    let step = crate::timeline::Step::begin("step", &format!("unpack {total} packages in {count} shards"));
    crate::tprintln!("  unpacking {total} packages in {count} shards, {running} at a time (logs in {})", logs.display());

    let queue = Mutex::new(shards.into_iter().enumerate().collect::<VecDeque<_>>());
    let finished = Mutex::new(0usize);
    let failures = Mutex::new(Vec::<String>::new());
    let shard_files: Vec<PathBuf> = (0..count).map(|i| progress_dir.join(format!("{pid}-shard{i}.json"))).collect();
    let done = AtomicBool::new(false);
    let mut summary = Progress::begin(repo, "fedora-rpm-unpack shards", "packages");
    let what = format!("packages ({count} shards)");

    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..running)
            .map(|_| {
                scope.spawn(|| loop {
                    let next = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
                    let Some((index, shard)) = next else {
                        break;
                    };
                    let started = Instant::now();
                    let log = logs.join(format!("{pid}-shard{index}.log"));
                    let result =
                        run_shard(repo, recipe, dir, identity, root, &work, index, &shard, &shard_files[index], &log);
                    let seconds = crate::timeline::format_duration(started.elapsed().as_secs_f64());
                    match result {
                        Ok(()) => {
                            let mut finished = finished.lock().unwrap_or_else(|e| e.into_inner());
                            *finished += shard.len();
                            crate::tprintln!(
                                "  unpacked shard {}/{count} ({} packages) in {seconds}; {}/{total} done",
                                index + 1,
                                shard.len(),
                                *finished
                            );
                        }
                        Err(error) => {
                            crate::teprintln!(
                                "  shard {}/{count} failed after {seconds} (log: {}): {error:#}",
                                index + 1,
                                log.display()
                            );
                            failures
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .push(format!("shard {} ({}): {error:#}", index + 1, log.display()));
                        }
                    }
                })
            })
            .collect();
        // The shards' counts as one, for `cargo xtask progress`.
        let summary = &mut summary;
        let monitor = scope.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(500));
                let finished = *finished.lock().unwrap_or_else(|e| e.into_inner());
                let running: u64 = shard_files.iter().filter_map(|file| shard_done(file)).sum();
                summary.update(finished as u64 + running, total as u64, &what, false);
            }
        });
        for handle in handles {
            if handle.join().is_err() {
                failures.lock().unwrap_or_else(|e| e.into_inner()).push("an unpack worker panicked".into());
            }
        }
        done.store(true, Ordering::SeqCst);
        let _ = monitor.join();
    });
    let finished = *finished.lock().unwrap_or_else(|e| e.into_inner());
    summary.update(finished as u64, total as u64, &what, true);
    summary.finish();
    for file in &shard_files {
        let _ = fs::remove_file(file);
    }
    let failures = failures.into_inner().unwrap_or_else(|e| e.into_inner());
    if !failures.is_empty() {
        bail!(
            "{} of {count} unpack shards failed ({finished} of {total} packages unpacked and kept):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
    step.finish();
    Ok(())
}

/// Removes the work directories (`<pid>-<shard>`) failed shards of builds
/// that are no longer running left behind.
fn remove_dead_shards(work: &Path) -> Result<()> {
    for entry in fs::read_dir(work).with_context(|| format!("listing {}", work.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let Some((pid, _)) = name.to_str().and_then(|name| name.split_once('-')) else {
            continue;
        };
        if pid.parse::<u32>().is_ok() && !Path::new("/proc").join(pid).exists() {
            if let Err(error) = fs::remove_dir_all(entry.path()) {
                crate::teprintln!("  warning: cannot remove {}: {error}", entry.path().display());
            }
        }
    }
    Ok(())
}

/// A shard's packages done, from its Recipe progress file.
fn shard_done(file: &Path) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_slice(&fs::read(file).ok()?).ok()?;
    (value["what"] == "packages" && value["state"] == "running").then(|| value["done"].as_u64()).flatten()
}

/// One shard: a view of its package files named by digest, one Recipe run of
/// the unpack recipe on it, then each package's tree into the store.
#[allow(clippy::too_many_arguments)]
fn run_shard(
    repo: &Path,
    recipe: &Path,
    dir: &Path,
    identity: &str,
    root: &str,
    work: &Path,
    index: usize,
    shard: &[Job],
    progress: &Path,
    log: &Path,
) -> Result<()> {
    (|| {
        let views = store(repo).join("views");
        fs::create_dir_all(&views)?;
        let view = tempfile::Builder::new().prefix("unpack-").tempdir_in(&views)?;
        for job in shard {
            let to = view.path().join(&job.row.sha256);
            if fs::hard_link(&job.file, &to).is_err() {
                fs::copy(&job.file, &to).with_context(|| format!("copying '{}' into the view", job.file.display()))?;
            }
        }
        let work_dir = work.join(format!("{}-{index}", std::process::id()));
        if work_dir.exists() {
            fs::remove_dir_all(&work_dir).with_context(|| format!("removing {}", work_dir.display()))?;
        }
        fs::create_dir_all(&work_dir).with_context(|| format!("creating {}", work_dir.display()))?;
        // Its publication lands in the work directory too, and goes with it.
        let ctx = Invocation::new(repo, "install", recipe, &work_dir)
            .working_directory(&work_dir)
            .input("packages", view.path())
            .concurrent(progress)
            .log_file(log)
            .run()?;
        let trees = ctx.directory("trees")?;
        for job in shard {
            store_tree(&trees.join(&job.row.sha256), dir, job, identity, root)
                .with_context(|| format!("moving {} into the unpack store", job.row.id))?;
        }
        drop(view);
        fs::remove_dir_all(&work_dir).with_context(|| format!("removing {}", work_dir.display()))?;
        Ok(())
    })()
}

/// Moves the unpacked package `from` (`root/`, `members.json`) into the store
/// `dir` as `job`'s entry: assembled in a temp directory beside it, then
/// renamed into place.
fn store_tree(from: &Path, dir: &Path, job: &Job, identity: &str, root: &str) -> Result<()> {
    let dest = dir.join(&job.key);
    if is_unpacked(&dest) {
        return Ok(());
    }
    ensure!(
        from.join("root").is_dir() && from.join("members.json").is_file(),
        "the unpack recipe left no tree for {} at {}",
        job.row.id,
        from.display()
    );
    let temp = dir.join(format!(".tmp-{}-{}", job.key, std::process::id()));
    if temp.exists() {
        fs::remove_dir_all(&temp)?;
    }
    fs::create_dir(&temp)?;
    fs::rename(from.join("root"), temp.join("root"))?;
    fs::rename(from.join("members.json"), temp.join("members.json"))?;
    let record = UnpackedRecord {
        format: UNPACK_FORMAT,
        key: &job.key,
        name: &job.row.name,
        id: &job.row.id,
        sha256: &job.row.sha256,
        recipe: identity,
        root,
        recipe_engine: levitate_recipe::identity::ENGINE_BEHAVIOUR,
        unpacked_at: distro_contract::runs::now_rfc3339(),
    };
    fs::write(temp.join("unpacked.json"), serde_json::to_vec_pretty(&record)?)?;
    if dest.exists() {
        // An unfinished entry from a build that died: replaced under the lock.
        fs::remove_dir_all(&dest)?;
    }
    fs::rename(&temp, &dest)?;
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

/// Where the last package to put a non-directory at a path got it from.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Owner {
    path: String,
    package: String,
    kind: String,
    /// A file: the store file its bytes came from; a link: its target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    origin: Option<String>,
    /// The SHA-256 of its metadata entry.
    entry: String,
}

/// Two packages put a non-directory at the same path; the later one won, as
/// it does in sequential extraction.
#[derive(Debug, Clone, Serialize)]
struct Conflict {
    path: String,
    earlier: String,
    later: String,
    /// Same kind, bytes (or link target) and metadata.
    identical: bool,
}

/// `composed.json`.
#[derive(Serialize)]
struct ComposedRecord<'a> {
    format: u32,
    key: &'a str,
    base: Option<&'a str>,
    packages: Vec<(&'a str, &'a str, &'a str)>,
    unpack_keys: &'a [String],
    conflicts: usize,
    differing_conflicts: usize,
    conflict_list: &'a [Conflict],
    composed_at: String,
}

fn is_composed(dir: &Path) -> bool {
    dir.join("composed.json").is_file() && dir.join("packages.txt").is_file() && dir.join("root").is_dir()
}

/// The composed tree of `layer` over `base` (nothing: an empty root), from
/// the unpack store, in order. Made once per key; reused after.
pub(crate) fn compose(repo: &Path, base: Option<&Composed>, layer: &[Unpacked]) -> Result<Composed> {
    let mut packages = base.map(|base| base.packages.clone()).unwrap_or_default();
    let mut unpack_keys = base.map(|base| base.unpack_keys.clone()).unwrap_or_default();
    packages.extend(layer.iter().map(|package| package.row.clone()));
    unpack_keys.extend(layer.iter().map(|package| package.key.clone()));
    let format = COMPOSE_FORMAT.to_string();
    let mut fields = vec![("store", "composed"), ("format", format.as_str())];
    fields.extend(unpack_keys.iter().map(|key| ("unpacked", key.as_str())));
    let key = key(&fields);
    let parent = store(repo).join("composed");
    fs::create_dir_all(&parent).with_context(|| format!("creating {}", parent.display()))?;
    let dir = parent.join(&key);
    let composed = Composed { key: key.clone(), dir: dir.clone(), packages, unpack_keys };
    let what = match base {
        None => format!("{} packages", layer.len()),
        Some(base) => format!("{} packages over the {} of {}", layer.len(), base.packages.len(), &base.key[..12]),
    };
    if is_composed(&dir) {
        crate::tprintln!("  composed tree {} ({what}) reused", &key[..12]);
        return Ok(composed);
    }
    let _locks = lock_buckets(&parent, std::iter::once(key.as_str()), "composing")?;
    if is_composed(&dir) {
        crate::tprintln!("  composed tree {} ({what}) reused", &key[..12]);
        return Ok(composed);
    }
    remove_unfinished(&parent, &key)?;
    let step = crate::timeline::Step::begin("step", &format!("compose {what}"));
    crate::tprintln!("  composing tree {} ({what})", &key[..12]);
    let started = Instant::now();
    let temp = parent.join(format!(".tmp-{key}-{}", std::process::id()));
    let root = temp.join("root");
    fs::create_dir_all(&root).with_context(|| format!("creating {}", root.display()))?;
    let result = compose_into(repo, &root, base, layer).and_then(|(owners, conflicts)| {
        write_owners(&temp.join("owners.jsonl"), &owners)?;
        fs::write(temp.join("packages.txt"), packages_text(&composed.packages))?;
        let differing = conflicts.iter().filter(|c| !c.identical).count();
        let record = ComposedRecord {
            format: COMPOSE_FORMAT,
            key: &key,
            base: base.map(|base| base.key.as_str()),
            packages: composed
                .packages
                .iter()
                .map(|row| (row.name.as_str(), row.id.as_str(), row.sha256.as_str()))
                .collect(),
            unpack_keys: &composed.unpack_keys,
            conflicts: conflicts.len(),
            differing_conflicts: differing,
            conflict_list: &conflicts,
            composed_at: distro_contract::runs::now_rfc3339(),
        };
        fs::write(temp.join("composed.json"), serde_json::to_vec_pretty(&record)?)?;
        report_conflicts(&conflicts, &dir);
        Ok(())
    });
    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temp);
        return Err(error.context(format!("composing the tree {}", &key[..12])));
    }
    fs::rename(&temp, &dir).with_context(|| format!("moving the composed tree into {}", dir.display()))?;
    fs::File::open(&parent)?.sync_all()?;
    crate::tprintln!(
        "  composed tree {} in {}",
        &key[..12],
        crate::timeline::format_duration(started.elapsed().as_secs_f64())
    );
    step.finish();
    Ok(composed)
}

/// `packages.txt`: `<sha256> <id>` per package, in order.
fn packages_text(rows: &[LockRow]) -> String {
    rows.iter().map(|row| format!("{} {}\n", row.sha256, row.id)).collect()
}

/// Composes into the empty `root`: the base tree's entries (its files linked)
/// and manifest, then each of `layer`'s packages replayed in order. Returns
/// who owns each non-directory path and the conflicts on the way.
fn compose_into(repo: &Path, root: &Path, base: Option<&Composed>, layer: &[Unpacked]) -> Result<(Vec<Owner>, Vec<Conflict>)> {
    let mut manifest = Manifest::default();
    let mut owners: HashMap<String, Owner> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    if let Some(base) = base {
        clone_tree(&base.root(), root)?;
        manifest = target::load(&base.root())?;
        for owner in read_owners(&base.dir.join("owners.jsonl"))? {
            order.push(owner.path.clone());
            owners.insert(owner.path.clone(), owner);
        }
    }
    let mut conflicts = Vec::new();
    let mut progress = Progress::begin(repo, "compose", "packages");
    let total = layer.len() as u64;
    for (done, package) in layer.iter().enumerate() {
        progress.update(done as u64, total, "packages composed", false);
        let members = target::load_members(&package.dir.join("members.json"))
            .with_context(|| format!("reading the extraction record of {}", package.row.id))?;
        let placed = target::replay(&package.dir.join("root"), &members, root, &mut manifest, true)
            .with_context(|| format!("composing {}", package.row.id))?;
        for placed in placed {
            let owner = Owner {
                path: placed.path.clone(),
                package: package.row.id.clone(),
                kind: placed.entry.kind.clone(),
                origin: match placed.entry.kind.as_str() {
                    "file" => placed.source.as_ref().map(|source| source.display().to_string()),
                    _ => placed.entry.link.clone(),
                },
                entry: sha256_hex(serde_json::to_vec(&placed.entry)?),
            };
            match owners.insert(placed.path.clone(), owner.clone()) {
                Some(earlier) if earlier.package != owner.package => conflicts.push(Conflict {
                    path: placed.path,
                    identical: same(&earlier, &owner),
                    earlier: earlier.package,
                    later: owner.package,
                }),
                Some(_) => {}
                None => order.push(placed.path),
            }
        }
    }
    progress.update(total, total, "packages composed", true);
    progress.finish();
    target::save(root, &manifest)?;
    // Paths a later member replaced with a directory are no longer owned.
    let owners = order
        .into_iter()
        .filter_map(|path| owners.remove(&path))
        .filter(|owner| fs::symlink_metadata(root.join(&owner.path)).is_ok_and(|m| !m.is_dir()))
        .collect();
    Ok((owners, conflicts))
}

/// Whether two owners of one path put the same thing there.
fn same(a: &Owner, b: &Owner) -> bool {
    if a.kind != b.kind || a.entry != b.entry {
        return false;
    }
    match (a.kind.as_str(), &a.origin, &b.origin) {
        ("file", Some(x), Some(y)) => x == y || file_digest(Path::new(x)).ok() == file_digest(Path::new(y)).ok(),
        (_, x, y) => x == y,
    }
}

fn file_digest(path: &Path) -> Result<String> {
    Ok(crate::parts::file_sha256(path)?.0)
}

fn report_conflicts(conflicts: &[Conflict], dir: &Path) {
    if conflicts.is_empty() {
        return;
    }
    let differing: Vec<&Conflict> = conflicts.iter().filter(|c| !c.identical).collect();
    crate::tprintln!(
        "  warning: {} paths are in more than one package ({} with different contents or metadata; \
         the later package's wins, as in sequential extraction); all are listed in {}",
        conflicts.len(),
        differing.len(),
        dir.join("composed.json").display()
    );
    for conflict in differing.iter().take(20) {
        crate::tprintln!("    /{}: {} then {}", conflict.path, conflict.earlier, conflict.later);
    }
    if differing.len() > 20 {
        crate::tprintln!("    ... and {} more", differing.len() - 20);
    }
}

fn write_owners(path: &Path, owners: &[Owner]) -> Result<()> {
    let mut out = std::io::BufWriter::new(fs::File::create(path)?);
    for owner in owners {
        serde_json::to_writer(&mut out, owner)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;
    Ok(())
}

fn read_owners(path: &Path) -> Result<Vec<Owner>> {
    let file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    std::io::BufReader::new(file)
        .lines()
        .filter(|line| line.as_ref().map_or(true, |line| !line.is_empty()))
        .map(|line| Ok(serde_json::from_str(&line?)?))
        .collect()
}

/// `src`'s entries recreated under the empty `dst`: directories with their
/// staging modes, links, and files as hardlinks to the same inodes (copies
/// across filesystems), so files hardlinked together stay so. The target
/// metadata manifest is left out: the caller writes its own.
fn clone_tree(src: &Path, dst: &Path) -> Result<()> {
    let mut dirs = Vec::new();
    for item in walkdir::WalkDir::new(src).follow_links(false).min_depth(1).sort_by_file_name() {
        let item = item?;
        let rel = item.path().strip_prefix(src)?;
        if rel == Path::new(target::MANIFEST) {
            continue;
        }
        let to = dst.join(rel);
        let md = fs::symlink_metadata(item.path())?;
        if md.is_dir() {
            fs::create_dir(&to)?;
            fs::set_permissions(&to, fs::Permissions::from_mode(0o700))?;
            dirs.push((to, md.permissions().mode() & 0o7777));
        } else if md.file_type().is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(item.path())?, &to)?;
        } else if md.is_file() {
            if let Err(error) = fs::hard_link(item.path(), &to) {
                if error.raw_os_error() != Some(libc::EXDEV) {
                    return Err(error).with_context(|| format!("linking {}", to.display()));
                }
                fs::copy(item.path(), &to)?;
                fs::set_permissions(&to, fs::Permissions::from_mode(md.permissions().mode() & 0o7777))?;
            }
        } else {
            bail!("unsupported inode in a composed tree: {}", item.path().display());
        }
    }
    for (dir, mode) in dirs.iter().rev() {
        fs::set_permissions(dir, fs::Permissions::from_mode(*mode))?;
    }
    Ok(())
}

/// Removes what earlier compositions of `key` left when they died; only
/// under the key's lock.
pub(crate) fn remove_unfinished(parent: &Path, key: &str) -> Result<()> {
    let prefix = format!(".tmp-{key}-");
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if entry.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)) {
            fs::remove_dir_all(entry.path()).with_context(|| format!("removing {}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// A recipe's `composed` input for `composed`: a fresh directory holding
/// `tree.tar` (made on first use) and `packages.txt`, linked from the store.
pub(crate) fn input_view(repo: &Path, composed: &Composed) -> Result<tempfile::TempDir> {
    let archive = composed.dir.join("tree.tar");
    if !archive.is_file() {
        let parent = composed.dir.parent().context("a composed tree has a parent")?;
        let _locks = lock_buckets(parent, std::iter::once(composed.key.as_str()), "archiving")?;
        if !archive.is_file() {
            let step = crate::timeline::Step::begin("step", &format!("archive the composed tree {}", &composed.key[..12]));
            crate::tprintln!("  archiving the composed tree {}", &composed.key[..12]);
            let mut temp = tempfile::Builder::new().prefix(".tree-").suffix(".tar").tempfile_in(&composed.dir)?;
            {
                let mut out = std::io::BufWriter::new(temp.as_file_mut());
                target::write_tree(&composed.root(), &mut out, false)
                    .with_context(|| format!("archiving {}", composed.root().display()))?;
                out.flush()?;
            }
            temp.as_file().sync_all()?;
            temp.persist(&archive)?;
            step.finish();
        }
    }
    view(repo, &[("tree.tar", archive.as_path()), ("packages.txt", &composed.dir.join("packages.txt"))])
}

/// A recipe's `composed` input for a tree archive made elsewhere (AcornOS's
/// base tree) and the packages it holds.
pub(crate) fn archive_view(repo: &Path, archive: &Path, rows: &[LockRow]) -> Result<tempfile::TempDir> {
    let view = view(repo, &[("tree.tar", archive)])?;
    fs::write(view.path().join("packages.txt"), packages_text(rows))?;
    Ok(view)
}

/// A fresh directory under `.artifacts/store/views` with `files` linked (or
/// copied, across filesystems) under their names.
fn view(repo: &Path, files: &[(&str, &Path)]) -> Result<tempfile::TempDir> {
    let views = store(repo).join("views");
    fs::create_dir_all(&views).with_context(|| format!("creating {}", views.display()))?;
    let view = tempfile::Builder::new().prefix("composed-").tempdir_in(&views)?;
    for (name, from) in files {
        let to = view.path().join(name);
        if fs::hard_link(from, &to).is_err() {
            fs::copy(from, &to).with_context(|| format!("copying '{}' into the view", from.display()))?;
        }
    }
    Ok(view)
}

/// Exclusive locks on the store `dir`'s buckets the `keys` fall in (the first
/// two hex digits, `.locks/<xx>.lock`), taken in order so two builds never
/// wait on each other in a circle. Held until the returned files drop.
fn lock_buckets<'a>(dir: &Path, keys: impl Iterator<Item = &'a str>, doing: &str) -> Result<Vec<fs::File>> {
    let locks = dir.join(".locks");
    fs::create_dir_all(&locks).with_context(|| format!("creating {}", locks.display()))?;
    let buckets: BTreeSet<&str> = keys.map(|key| &key[..2]).collect();
    let mut held = Vec::new();
    let mut said = false;
    for bucket in buckets {
        let path = locks.join(format!("{bucket}.lock"));
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        if !flock(&file, libc::LOCK_EX | libc::LOCK_NB)? {
            if !said {
                crate::tprintln!("  waiting for another build {doing} in {}", dir.display());
                said = true;
            }
            flock(&file, libc::LOCK_EX)?;
        }
        held.push(file);
    }
    Ok(held)
}

/// `flock(operation)`: false when a non-blocking lock is held elsewhere.
pub(crate) fn flock(file: &fs::File, operation: libc::c_int) -> Result<bool> {
    loop {
        // SAFETY: flock only reads the descriptor of the open lock file.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EWOULDBLOCK) if operation & libc::LOCK_NB != 0 => return Ok(false),
            _ => return Err(error).context("locking the store"),
        }
    }
}

/// A progress file in Recipe's `--progress-file` format for a step the
/// builder does itself (composition, the unpack shards together), recorded
/// as the active recipe so `cargo xtask progress` shows its count. Best
/// effort: a file that cannot be written never fails a build.
struct Progress {
    path: PathBuf,
    name: String,
    started: String,
    last: Option<Instant>,
    active: bool,
}

impl Progress {
    fn begin(repo: &Path, name: &str, what: &str) -> Self {
        let slug: String = name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let path = crate::timeline::progress_file(repo, &format!("builder-{slug}.json"));
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut progress = Self {
            path,
            name: name.into(),
            started: distro_contract::runs::now_rfc3339(),
            last: None,
            active: true,
        };
        progress.write(0, 0, what, "running");
        crate::timeline::recipe_begin(name, &progress.path);
        progress
    }

    fn update(&mut self, done: u64, total: u64, what: &str, force: bool) {
        if force || self.last.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
            self.write(done, total, what, "running");
        }
    }

    fn write(&mut self, done: u64, total: u64, what: &str, state: &str) {
        self.last = Some(Instant::now());
        let record = serde_json::json!({
            "version": 1,
            "invocation": format!("builder-{}", std::process::id()),
            "recipe": self.name,
            "phase": "build",
            "what": what,
            "done": done,
            "total": total,
            "started": self.started,
            "updated": distro_contract::runs::now_rfc3339(),
            "state": state,
        });
        let _ = distro_contract::runs::write_atomic(&self.path, record.to_string().as_bytes());
    }

    fn finish(&mut self) {
        if self.active {
            self.active = false;
            crate::timeline::recipe_end();
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.finish();
    }
}

/// The AcornOS base tree: `alpine-apk-base-tree.rhai` on the base lock's
/// APKs (`apks`, the base preseed's output), in a work directory of its own,
/// so Recipe's saved state reuses it while that recipe, the lock and the APKs
/// are unchanged. Returns the tree's archive.
pub(crate) fn apk_base_tree(repo: &Path, distro: &str, apks: &Path) -> Result<PathBuf> {
    let recipe = repo.join(APK_BASE_TREE_RECIPE);
    let work = repo.join(".artifacts/work/apk-base-tree").join(distro);
    fs::create_dir_all(&work).with_context(|| format!("creating {}", work.display()))?;
    Invocation::new(repo, "install", &recipe, &work)
        .input("apks", apks)
        .run()
        .context("installing the AcornOS base tree")?
        .file("tree")
}

/// The recipe that makes AcornOS's base tree.
pub(crate) const APK_BASE_TREE_RECIPE: &str = "distro/builder/os-recipes/alpine-apk-base-tree.rhai";

/// The packages the Fedora apps-build root takes: no firmware or microcode
/// (`wanted` in `fedora-rpm-apps-build-root.rhai`, which checks the list).
pub(crate) fn apps_build_wanted(row: &LockRow) -> bool {
    !row.name.contains("firmware") && !row.name.contains("microcode")
}

