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

/// Where the pinned compiler image lives, and the file that pins it.
const COMPILER_IMAGE_DIR: &str = ".artifacts/ginux-env/prepared-image-build-tools";
const COMPILER_IMAGE_PIN: &str = "tools/recipe/bootstrap/product-ci.json";

/// The compiler image OS builds run Recipe in, as `(GINUX_ROOT, GINUX_ROOTFS)`:
/// the one `product-ci.json` pins (`oci:recipe-compiler@sha256:<image_sha256>`),
/// produced at `.artifacts/ginux-env/prepared-image-build-tools/rootfs.tar`, after
/// checking its `provenance.json` carries the pinned `sha256`. `None` when the
/// builder's environment sets `GINUX_ROOT` or `GINUX_ROOTFS`: those keep winning.
/// The builder never produces the image.
fn compiler_image(repo: &Path) -> Result<Option<(String, PathBuf)>> {
    if std::env::var_os("GINUX_ROOT").is_some() || std::env::var_os("GINUX_ROOTFS").is_some() {
        return Ok(None);
    }
    let pin_file = repo.join(COMPILER_IMAGE_PIN);
    let pin: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&pin_file).with_context(|| format!("reading '{}'", pin_file.display()))?,
    )
    .with_context(|| format!("parsing '{}'", pin_file.display()))?;
    let digest = pin["image_sha256"]
        .as_str()
        .with_context(|| format!("'{}' has no image_sha256", pin_file.display()))?;
    let dir = repo.join(COMPILER_IMAGE_DIR);
    let (rootfs, provenance) = (dir.join("rootfs.tar"), dir.join("provenance.json"));
    let missing = |why: String| {
        anyhow::anyhow!(
            "the compiler image oci:recipe-compiler@sha256:{digest} (pinned in {COMPILER_IMAGE_PIN}) is {why}; \
             produce it as tools/recipe/docs/setup-linux.md describes (expected {} and {})",
            rootfs.display(),
            provenance.display()
        )
    };
    if !rootfs.is_file() {
        return Err(missing(format!("not at '{}'", rootfs.display())));
    }
    let recorded: serde_json::Value = match std::fs::read(&provenance) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("parsing '{}'", provenance.display()))?,
        Err(_) => return Err(missing(format!("without '{}'", provenance.display()))),
    };
    match recorded["sha256"].as_str() {
        Some(found) if found == digest => {}
        found => {
            return Err(missing(format!(
                "not what '{}' records (sha256 {})",
                provenance.display(),
                found.unwrap_or("absent")
            )))
        }
    }
    Ok(Some((format!("oci:recipe-compiler@sha256:{digest}"), rootfs)))
}

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
    crate::teprintln!("  building ginux (cargo build -p ginux-init --bin ginux --release in {GINUX_DIR})");
    let step = crate::timeline::Step::begin("tool", "cargo build ginux");
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
    step.finish();
    Ok(BINARY.get_or_init(|| path))
}

/// `cargo build -p levitate-recipe` with the builder's profile.
fn build(repo: &Path) -> Result<PathBuf> {
    let release = !cfg!(debug_assertions);
    crate::teprintln!(
        "  building recipe (cargo build -p levitate-recipe{})",
        if release { " --release" } else { "" }
    );
    let step = crate::timeline::Step::begin("tool", "cargo build recipe");
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
    step.finish();
    Ok(path)
}

/// One run of a recipe phase (`install`, `isinstalled`, ...) in its work directory;
/// imports resolve from `<repo>/distro/builder/os-recipes`.
pub struct Invocation<'a> {
    repo: &'a Path,
    phase: &'static str,
    recipe: PathBuf,
    work: PathBuf,
    args: Vec<OsString>,
    root: Option<(String, PathBuf)>,
    worker: Option<PathBuf>,
    cwd: Option<PathBuf>,
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
            root: None,
            worker: None,
            cwd: None,
        }
    }

    /// Keep short-lived verification outputs in the invocation's private scope,
    /// so Recipe's exported artifacts are removed with that scope.
    pub fn working_directory(mut self, directory: &Path) -> Self {
        self.cwd = Some(directory.to_path_buf());
        self
    }

    /// Runs this recipe in another root than Recipe's usual one: the image
    /// `rootfs` (an assembled root tar), named by `reference`
    /// (`oci:NAME@sha256:DIGEST`, the digest of that tar). The desktop apps
    /// are compiled this way, in a root made of the variant's own packages.
    pub fn root(mut self, reference: impl Into<String>, rootfs: impl Into<PathBuf>) -> Self {
        self.root = Some((reference.into(), rootfs.into()));
        self
    }

    /// The Recipe binary that runs inside the root, when the builder's own
    /// cannot: a root with another C library needs the static one.
    pub fn worker(mut self, worker: impl Into<PathBuf>) -> Self {
        self.worker = Some(worker.into());
        self
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
        let recipe_name = recipe
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| recipe.display().to_string());
        let step = crate::timeline::Step::begin("recipe", &format!("recipe {} {recipe_name}", self.phase));
        crate::teprintln!(
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
        // OS producers and their imports use the builder's collection;
        // the workspace's user-managed recipe catalog is a separate consumer.
        let os_recipes_path = self.repo.join("distro/builder/os-recipes");

        let mut cmd = Cmd::new(bin)
            // OS composition consumes products without activating host packages.
            .arg("--no-activate")
            .arg(self.phase)
            .arg(recipe)
            .arg("--build-dir")
            .arg(work)
            .arg("--state-dir")
            .arg(work.join("recipe-state"))
            .arg("--json-output")
            .arg(result.path())
            .arg("--recipes-path")
            .arg(&os_recipes_path)
            .args(&self.args)
            .dir(self.cwd.as_deref().unwrap_or(self.repo))
            .stdout_to_stderr()
            .error_msg(format!("recipe {} {}", self.phase, recipe.display()));
        // Recipe keeps this file current (done/total of what it is doing); the
        // builder records its path, so `cargo xtask progress` can read it.
        let progress = (self.phase == "install").then(|| crate::timeline::progress_file(self.repo, &recipe_name));
        let mut _active = None;
        if let Some(file) = &progress {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            cmd = cmd.arg("--progress-file").arg(file);
            crate::timeline::recipe_begin(&recipe_name, file);
            _active = Some(ActiveRecipe);
        }
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            let control = name.starts_with("RECIPE_") || name.starts_with("GINUX_");
            if control && !LAUNCHER_ENV.contains(&name.as_ref()) {
                cmd = cmd.env_remove(&key);
            }
        }
        let ginux = ginux_binary(self.repo)?;
        let worker = self.worker.as_deref().unwrap_or(bin);
        cmd = cmd.env("GINUX_BIN", ginux).env("RECIPE_WORKER_BIN", worker);
        for (name, value) in [("GINUX_BIN", ginux), ("RECIPE_WORKER_BIN", worker)] {
            if let Some(ambient) = std::env::var_os(name).filter(|ambient| Path::new(ambient) != value) {
                crate::teprintln!("    {name}={} (the builder's own; ignoring {})", value.display(), Path::new(&ambient).display());
            } else {
                crate::teprintln!("    {name}={}", value.display());
            }
        }
        for name in LAUNCHER_ENV {
            // This invocation's own root replaces the ambient one.
            if self.root.is_some() && name.starts_with("GINUX_ROOT") {
                continue;
            }
            if let Some(value) = std::env::var_os(name) {
                crate::teprintln!("    {name}={}", Path::new(&value).display());
            }
        }
        if let Some((reference, rootfs)) = &self.root {
            cmd = cmd.env("GINUX_ROOT", reference).env("GINUX_ROOTFS", rootfs);
            crate::teprintln!("    GINUX_ROOT={reference} (this recipe's own root)");
            crate::teprintln!("    GINUX_ROOTFS={}", rootfs.display());
        } else if let Some((reference, rootfs)) = compiler_image(self.repo)? {
            cmd = cmd.env("GINUX_ROOT", &reference).env("GINUX_ROOTFS", &rootfs);
            crate::teprintln!("    GINUX_ROOT={reference} (the pinned compiler image)");
            crate::teprintln!("    GINUX_ROOTFS={}", rootfs.display());
        }
        cmd.run_interactive()?;

        let bytes = std::fs::read(result.path())
            .context("reading this invocation's recipe JSON result")?;
        let result = RecipeResult::from_json(&bytes)?;
        if let Some(file) = &progress {
            let _ = std::fs::remove_file(file);
        }
        step.finish();
        Ok(result)
    }
}

/// Clears the recorded Recipe invocation when the run ends, however it ends.
struct ActiveRecipe;

impl Drop for ActiveRecipe {
    fn drop(&mut self) {
        crate::timeline::recipe_end();
    }
}
