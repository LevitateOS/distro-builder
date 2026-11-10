//! Running Recipe, which runs every producer of both distros.
//!
//! [`binary`] resolves the `recipe` binary once per process: `RECIPE_BIN` when set,
//! else the workspace's `levitate-recipe`, built with the builder's own profile into
//! Cargo's target directory. So the spawned binary and the `levitate_recipe` library
//! the builder links come from the same source. [`Invocation`] runs one recipe phase
//! under the [`crate::process`] policy and returns its [`RecipeResult`].

pub mod linux;
mod outputs;
pub mod rootfs_source;
pub mod tools;

pub use crate::pipeline::paths::recipe_work_dir;
pub use outputs::RecipeResult;

use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::process::Cmd;

/// Recipe's settings for its image and frame timeout, passed to every invocation as
/// the builder got them. The launcher (`GINUX_BIN`) and the worker
/// (`RECIPE_WORKER_BIN`) are the builder's own: the Ginux it builds from
/// `tools/ginux` and the Recipe binary it runs, so an invocation never runs an older
/// launcher or worker than the checkout. Every other `RECIPE_*`/`GINUX_*` variable
/// is removed, so an ambient one (a worker's `RECIPE_WORKER`, say) cannot change how
/// Recipe runs.
const LAUNCHER_ENV: &[&str] = &["GINUX_ROOT", "GINUX_ROOTFS", "RECIPE_FRAME_TIMEOUT_SECS"];

/// The Ginux crate's directory, the cargo target it builds for, and its binary.
const GINUX_DIR: &str = "tools/ginux";
const GINUX_TARGET: &str = "x86_64-unknown-linux-gnu";

/// The `recipe` binary: `RECIPE_BIN` if set, else the workspace build (built on
/// first use).
pub fn binary(repo: &Path) -> Result<&'static Path> {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = BINARY.get() {
        return Ok(path);
    }
    let path = match std::env::var_os("RECIPE_BIN") {
        Some(path) => {
            let path = PathBuf::from(path);
            let metadata = std::fs::metadata(&path)
                .with_context(|| format!("RECIPE_BIN '{}'", path.display()))?;
            if !metadata.is_file() {
                bail!("RECIPE_BIN '{}' is not a file", path.display());
            }
            path
        }
        None => build(repo)?,
    };
    Ok(BINARY.get_or_init(|| path))
}

/// The Ginux launcher every invocation runs: `tools/ginux`'s `ginux` binary,
/// built (release, as its Makefile builds it) on first use in each process, so it
/// is always the checkout's.
pub fn ginux_binary(repo: &Path) -> Result<&'static Path> {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = BINARY.get() {
        return Ok(path);
    }
    let dir = repo.join(GINUX_DIR);
    // Beside the builder's own builds when they have a target directory of their
    // own (a build run as root keeps root's files out of tools/ginux).
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(|target| PathBuf::from(target).join("ginux"))
        .unwrap_or_else(|| dir.join("target"));
    eprintln!("  building ginux (cargo build -p ginux-init --bin ginux --release in {GINUX_DIR})");
    Cmd::new("cargo")
        .args(["build", "-q", "-p", "ginux-init", "--bin", "ginux", "--release", "--target", GINUX_TARGET])
        .arg("--target-dir")
        .arg(&target_dir)
        .dir(&dir)
        .stdout_to_stderr()
        .error_msg("building ginux")
        .run_interactive()?;
    let path = target_dir.join(GINUX_TARGET).join("release/ginux");
    if !path.is_file() {
        bail!("cargo built ginux, but it is not at '{}'", path.display());
    }
    Ok(BINARY.get_or_init(|| path))
}

/// `cargo build -p levitate-recipe` with the builder's profile.
fn build(repo: &Path) -> Result<PathBuf> {
    let release = !cfg!(debug_assertions);
    eprintln!(
        "  building recipe (cargo build -p levitate-recipe{})",
        if release { " --release" } else { "" }
    );
    let mut cargo = Cmd::new("cargo")
        .args(["build", "-q", "-p", "levitate-recipe", "--bin", "recipe"])
        .dir(repo)
        .stdout_to_stderr()
        .error_msg("building recipe");
    if release {
        cargo = cargo.arg("--release");
    }
    cargo.run_interactive()?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target"));
    let path = target
        .join(if release { "release" } else { "debug" })
        .join("recipe");
    if !path.is_file() {
        bail!("cargo built recipe, but it is not at '{}'", path.display());
    }
    Ok(path)
}

/// One run of a recipe phase (`install`, `isinstalled`, ...) in its work directory;
/// imports resolve from `<repo>/distro/builder/recipes`.
pub struct Invocation<'a> {
    repo: &'a Path,
    phase: &'static str,
    recipe: PathBuf,
    work: PathBuf,
    args: Vec<OsString>,
}

impl<'a> Invocation<'a> {
    pub fn new(
        repo: &'a Path,
        phase: &'static str,
        recipe: impl Into<PathBuf>,
        work: impl Into<PathBuf>,
    ) -> Self {
        Self {
            repo,
            phase,
            recipe: recipe.into(),
            work: work.into(),
            args: Vec::new(),
        }
    }

    fn flag(mut self, flag: &str, value: impl Into<OsString>) -> Self {
        self.args.push(flag.into());
        self.args.push(value.into());
        self
    }

    pub fn define(self, key: &str, value: &str) -> Self {
        self.flag("--define", format!("{key}={value}"))
    }

    /// A read-only file or tree the recipe reads as input `name`.
    pub fn input(self, name: &str, path: &Path) -> Self {
        let mut value = OsString::from(format!("{name}="));
        value.push(path);
        self.flag("--input", value)
    }

    /// Acquire the sources again, even when Recipe holds them already.
    pub fn force_acquire(mut self, force: bool) -> Self {
        if force {
            self.args.push("--force-acquire".into());
        }
        self
    }

    /// Other Recipe flags, passed as they are (the kernel's `--force-build`, `--autofix`).
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn run(self) -> Result<RecipeResult> {
        let recipe = &self.recipe;
        let work = &self.work;
        eprintln!(
            "  recipe {} {} (work: {})",
            self.phase,
            recipe.display(),
            work.display()
        );
        let bin = binary(self.repo)?;
        std::fs::create_dir_all(work)
            .with_context(|| format!("creating recipe work directory '{}'", work.display()))?;
        // This invocation must write its own result: never read a stale one.
        let result = tempfile::Builder::new()
            .prefix(".recipe-result.")
            .suffix(".json")
            .tempfile_in(work)
            .context("creating a fresh recipe result file")?;
        // Every recipe and its imports live here, wherever the script itself is.
        let recipes_path = self.repo.join("distro/builder/recipes");

        let mut cmd = Cmd::new(bin)
            .arg(self.phase)
            .arg(recipe)
            .arg("--build-dir")
            .arg(work)
            .arg("--state-dir")
            .arg(work.join("recipe-state"))
            .arg("--json-output")
            .arg(result.path())
            .arg("--recipes-path")
            .arg(&recipes_path)
            .args(&self.args)
            .dir(self.repo)
            .stdout_to_stderr()
            .error_msg(format!("recipe {} {}", self.phase, recipe.display()));
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            let control = name.starts_with("RECIPE_") || name.starts_with("GINUX_");
            if control && !LAUNCHER_ENV.contains(&name.as_ref()) {
                cmd = cmd.env_remove(&key);
            }
        }
        let ginux = ginux_binary(self.repo)?;
        cmd = cmd.env("GINUX_BIN", ginux).env("RECIPE_WORKER_BIN", bin);
        for (name, value) in [("GINUX_BIN", ginux), ("RECIPE_WORKER_BIN", bin)] {
            if let Some(ambient) = std::env::var_os(name).filter(|ambient| Path::new(ambient) != value) {
                eprintln!("    {name}={} (the builder's own; ignoring {})", value.display(), Path::new(&ambient).display());
            } else {
                eprintln!("    {name}={}", value.display());
            }
        }
        for name in LAUNCHER_ENV {
            if let Some(value) = std::env::var_os(name) {
                eprintln!("    {name}={}", Path::new(&value).display());
            }
        }
        cmd.run_interactive()?;

        let bytes = std::fs::read(result.path())
            .context("reading this invocation's recipe JSON result")?;
        RecipeResult::from_json(&bytes)
    }
}
