//! The build's timeline: which step is running, how long each step took, and what
//! the build prints with the time on every line (`docs/build-redesign.md`, section 7).
//!
//! A release build calls [`start`] once with its run directory and [`finish`] at
//! its end. In between, every step is a [`Step`] (a guard) or one of a [`Seq`]
//! (steps that follow each other in one function). They nest: the open steps are
//! the stack `build > part > step`.
//!
//! - `<run directory>/timeline.jsonl`: one JSON line per step start and end
//!   ([`Event`]), appended as it happens.
//! - `<run directory>/timeline.txt`: the "where the time went" table, written
//!   at the end (and printed), also when the build failed.
//! - `.artifacts/progress/<distro>.json`: the current state ([`State`]): the
//!   step stack, the builder's pid and the active Recipe progress file. Replaced
//!   atomically at every change. `cargo xtask progress` reads it.
//! - `.artifacts/progress/<distro>/<recipe>-<pid>.json`: Recipe's own progress
//!   file of one invocation (`--progress-file`); [`progress_file`] names it.
//!
//! Everything here is best effort: a file that cannot be written never fails a
//! build. Printing goes through [`tprintln!`] and [`teprintln!`], which start
//! every line with the local time as `HH:MM:SS`.

use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// The version of the state and event formats.
pub const VERSION: u32 = 1;

/// `println!` with the local time (`HH:MM:SS`) at the start of every line.
#[macro_export]
macro_rules! tprintln {
    ($($arg:tt)*) => { $crate::timeline::print(false, format_args!($($arg)*)) };
}

/// `eprintln!` with the local time (`HH:MM:SS`) at the start of every line.
#[macro_export]
macro_rules! teprintln {
    ($($arg:tt)*) => { $crate::timeline::print(true, format_args!($($arg)*)) };
}

/// The local time of day, `HH:MM:SS`.
pub fn clock() -> String {
    clock_at(now_epoch())
}

/// The local time of day, `HH:MM:SS`, at `epoch` seconds.
pub fn clock_at(epoch: i64) -> String {
    // SAFETY: `localtime_r` only writes the structure passed to it.
    unsafe {
        let at: libc::time_t = epoch as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&at, &mut tm).is_null() {
            return "--:--:--".into();
        }
        format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
    }
}

/// Now, in seconds since the Unix epoch.
pub fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Seconds since the Unix epoch of an RFC 3339 UTC time (`2026-11-11T10:00:00Z`;
/// a fraction or an offset after the seconds is ignored).
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') {
        return None;
    }
    let num = |from: usize, to: usize| text.get(from..to)?.parse::<i64>().ok();
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // Civil date to days since 1970-01-01 (Howard Hinnant's algorithm).
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Prints `args` to stdout (or stderr), each line after the time.
pub fn print(to_stderr: bool, args: std::fmt::Arguments) {
    let time = clock();
    let text = args.to_string();
    let mut out = String::new();
    for line in text.split('\n') {
        let _ = writeln!(out, "{time} {line}");
    }
    if to_stderr {
        eprint!("{out}");
    } else {
        print!("{out}");
    }
}

/// `.artifacts/progress`.
pub fn progress_dir(repo: &Path) -> PathBuf {
    repo.join(".artifacts/progress")
}

/// The current-state file of `distro`.
pub fn state_path(repo: &Path, distro: &str) -> PathBuf {
    progress_dir(repo).join(format!("{distro}.json"))
}

/// One step of the stack in the state file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRef {
    pub name: String,
    /// `run`, `part`, `recipe`, `step`, `tool`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub started: String,
}

/// The Recipe invocation running now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRecipe {
    pub name: String,
    pub progress_file: String,
}

/// `.artifacts/progress/<distro>.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub version: u32,
    pub distro: String,
    pub product: String,
    pub run_dir: String,
    /// The builder's process id.
    pub pid: u32,
    /// `running`, `finished` or `failed`.
    pub state: String,
    pub started: String,
    pub updated: String,
    /// Outermost first; the first is the build itself (kind `run`).
    pub steps: Vec<StepRef>,
    pub recipe: Option<ActiveRecipe>,
}

/// One line of `timeline.jsonl`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// `start` or `end`.
    pub event: String,
    pub step: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// `reused` or `built` on a part.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How many steps were open around it.
    pub depth: usize,
    /// When the event happened (RFC 3339, UTC).
    pub at: String,
    /// End events: when the step started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// End events: how long it took.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// End events: whether it succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
}

struct Open {
    name: String,
    kind: String,
    key: Option<String>,
    note: Option<String>,
    started_wall: String,
    started: Instant,
}

struct Done {
    name: String,
    note: Option<String>,
    depth: usize,
    offset: f64,
    seconds: f64,
    ok: bool,
}

struct Active {
    repo: PathBuf,
    distro: String,
    product: String,
    dir: PathBuf,
    started_wall: String,
    started: Instant,
    stack: Vec<Open>,
    done: Vec<Done>,
    recipe: Option<ActiveRecipe>,
}

static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Active) -> R) -> Option<R> {
    let mut guard = ACTIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.as_mut().map(f)
}

fn now() -> String {
    distro_contract::runs::now_rfc3339()
}

impl Active {
    fn event(&self, event: &str, open: &Open, depth: usize, end: Option<(f64, bool)>) {
        let line = Event {
            event: event.into(),
            step: open.name.clone(),
            kind: open.kind.clone(),
            key: open.key.clone(),
            note: open.note.clone(),
            depth,
            at: now(),
            start: end.map(|_| open.started_wall.clone()),
            seconds: end.map(|(seconds, _)| seconds),
            ok: end.map(|(_, ok)| ok),
        };
        let Ok(mut text) = serde_json::to_string(&line) else {
            return;
        };
        text.push('\n');
        if let Ok(mut file) = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("timeline.jsonl"))
        {
            let _ = file.write_all(text.as_bytes());
        }
    }

    fn save(&self, state: &str) {
        let value = State {
            version: VERSION,
            distro: self.distro.clone(),
            product: self.product.clone(),
            run_dir: self.dir.display().to_string(),
            pid: std::process::id(),
            state: state.into(),
            started: self.started_wall.clone(),
            updated: now(),
            steps: self
                .stack
                .iter()
                .map(|open| StepRef {
                    name: open.name.clone(),
                    kind: open.kind.clone(),
                    key: open.key.clone(),
                    started: open.started_wall.clone(),
                })
                .collect(),
            recipe: self.recipe.clone(),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&value) {
            let _ = distro_contract::runs::write_replace(&state_path(&self.repo, &self.distro), &bytes);
        }
    }

    fn push(&mut self, kind: &str, name: &str) {
        let open = Open {
            name: name.into(),
            kind: kind.into(),
            key: None,
            note: None,
            started_wall: now(),
            started: Instant::now(),
        };
        self.event("start", &open, self.stack.len(), None);
        self.stack.push(open);
        self.save("running");
    }

    /// Closes the open steps from `depth` inward (innermost first).
    fn close_from(&mut self, depth: usize, ok: bool) {
        let mut changed = false;
        while self.stack.len() > depth {
            let open = self.stack.pop().expect("the stack is longer than depth");
            let seconds = open.started.elapsed().as_secs_f64();
            let level = self.stack.len();
            self.event("end", &open, level, Some((round(seconds), ok)));
            self.done.push(Done {
                offset: open.started.duration_since(self.started).as_secs_f64(),
                name: open.name,
                note: open.note,
                depth: level,
                seconds,
                ok,
            });
            changed = true;
        }
        if changed {
            self.save("running");
        }
    }
}

fn round(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

/// Begins recording the release build of `product` for `distro` in the run
/// directory `dir`. The build itself is the outermost step.
pub fn start(repo: &Path, distro: &str, product: &str, dir: &Path) {
    let mut active = Active {
        repo: repo.to_path_buf(),
        distro: distro.into(),
        product: product.into(),
        dir: dir.to_path_buf(),
        started_wall: now(),
        started: Instant::now(),
        stack: Vec::new(),
        done: Vec::new(),
        recipe: None,
    };
    active.push("run", &format!("{product} release of {distro}"));
    *ACTIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(active);
}

/// Ends the recording: closes every open step (as failed unless `ok`), marks the
/// current-state file finished or failed, prints the "where the time went" table
/// and writes it to `timeline.txt`.
pub fn finish(ok: bool) {
    let Some(active) = ACTIVE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take() else {
        return;
    };
    let mut active = active;
    active.close_from(0, ok);
    active.recipe = None;
    active.save(if ok { "finished" } else { "failed" });
    let table = table(&active, ok);
    print!("{}", prefixed(&table));
    let _ = fs::write(active.dir.join("timeline.txt"), table);
}

fn prefixed(text: &str) -> String {
    let time = clock();
    text.lines().map(|line| format!("{time} {line}\n")).collect()
}

/// A duration as `1h02m03s`, `4m05s` or `6.5s`.
pub fn format_duration(seconds: f64) -> String {
    let whole = seconds.max(0.0).round() as u64;
    if seconds < 10.0 {
        format!("{seconds:.1}s")
    } else if whole < 60 {
        format!("{whole}s")
    } else if whole < 3600 {
        format!("{}m{:02}s", whole / 60, whole % 60)
    } else {
        format!("{}h{:02}m{:02}s", whole / 3600, whole % 3600 / 60, whole % 60)
    }
}

fn table(active: &Active, ok: bool) -> String {
    let total = active.started.elapsed().as_secs_f64().max(0.001);
    let mut rows: Vec<&Done> = active.done.iter().collect();
    rows.sort_by(|a, b| a.offset.total_cmp(&b.offset).then(a.depth.cmp(&b.depth)));
    let label = |row: &Done| {
        let note = row.note.as_deref().map(|note| format!(" ({note})")).unwrap_or_default();
        let failed = if row.ok { "" } else { "  FAILED" };
        format!("{}{}{note}{failed}", "  ".repeat(row.depth), row.name)
    };
    let width = rows.iter().map(|row| label(row).chars().count()).max().unwrap_or(0).min(100);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "Where the time went: {} {} of {} ({})",
        active.product,
        if ok { "built" } else { "FAILED" },
        active.distro,
        format_duration(total)
    );
    let _ = writeln!(out, "  {:<width$}  {:>10}  {:>6}", "step", "time", "share");
    for row in rows {
        let _ = writeln!(
            out,
            "  {:<width$}  {:>10}  {:>5.1}%",
            label(row),
            format_duration(row.seconds),
            row.seconds / total * 100.0
        );
    }
    let _ = writeln!(out, "  (inner steps are part of the step around them, so shares add up per level)");
    out
}

/// One step: it ends when the guard is [`finish`](Step::finish)ed (ok) or
/// dropped without that (failed, as when a `?` returned).
pub struct Step {
    depth: usize,
}

impl Step {
    /// Starts a step of `kind` (`part`, `recipe`, `step`, `tool`). Does nothing
    /// when no build is being recorded.
    pub fn begin(kind: &str, name: &str) -> Self {
        let depth = with(|active| {
            let depth = active.stack.len();
            active.push(kind, name);
            depth
        })
        .unwrap_or(usize::MAX);
        Self { depth }
    }

    /// Sets the step's key (a part's short key), shown in the state and timeline.
    pub fn key(&self, key: &str) {
        self.update(|open| open.key = Some(key.into()));
    }

    /// Sets what happened (a part: `reused` or `built`), shown in the timeline.
    pub fn note(&self, note: &str) {
        self.update(|open| open.note = Some(note.into()));
    }

    fn update(&self, f: impl FnOnce(&mut Open)) {
        with(|active| {
            if let Some(open) = active.stack.get_mut(self.depth) {
                f(open);
                active.save("running");
            }
        });
    }

    /// Ends the step as succeeded.
    pub fn finish(self) {
        with(|active| active.close_from(self.depth, true));
    }
}

impl Drop for Step {
    fn drop(&mut self) {
        // A no-op once finished: the stack is no longer than `depth`.
        if self.depth != usize::MAX {
            with(|active| active.close_from(self.depth, false));
        }
    }
}

/// Steps that follow each other in one function: each [`step`](Seq::step)
/// ends the one before as succeeded; [`finish`](Seq::finish) ends the last one
/// as succeeded; dropping without it ends the last one as failed.
pub struct Seq {
    base: usize,
}

impl Seq {
    pub fn new() -> Self {
        let base = with(|active| active.stack.len()).unwrap_or(usize::MAX);
        Self { base }
    }

    /// Ends the previous step of this sequence and starts the next.
    pub fn step(&self, name: &str) {
        if self.base == usize::MAX {
            return;
        }
        with(|active| {
            active.close_from(self.base, true);
            active.push("step", name);
        });
    }

    /// Ends the last step as succeeded.
    pub fn finish(self) {
        if self.base != usize::MAX {
            with(|active| active.close_from(self.base, true));
        }
    }
}

impl Default for Seq {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Seq {
    fn drop(&mut self) {
        if self.base != usize::MAX {
            with(|active| active.close_from(self.base, false));
        }
    }
}

/// The progress file for one Recipe invocation of `recipe` (a file name):
/// `.artifacts/progress/<distro>/<recipe>-<pid>.json`. The distro is the
/// recorded build's, else `_adhoc`.
pub fn progress_file(repo: &Path, recipe: &str) -> PathBuf {
    let distro = with(|active| active.distro.clone()).unwrap_or_else(|| "_adhoc".into());
    let stem = Path::new(recipe)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| recipe.into());
    progress_dir(repo)
        .join(distro)
        .join(format!("{stem}-{}.json", std::process::id()))
}

/// Records that the Recipe invocation `name` reports to `file`.
pub fn recipe_begin(name: &str, file: &Path) {
    with(|active| {
        active.recipe = Some(ActiveRecipe {
            name: name.into(),
            progress_file: file.display().to_string(),
        });
        active.save("running");
    });
}

/// Records that no Recipe invocation is running.
pub fn recipe_end() {
    with(|active| {
        active.recipe = None;
        active.save("running");
    });
}
