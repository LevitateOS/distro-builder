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
mod stamp;
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
/// Recipe runs. An ambient `GINUX_MINIMAL_ROOT`/`GINUX_MINIMAL_ROOTFS` among them: an
/// invocation whose recipes ask for the minimal image (`NEEDS.runtime`) is given the one
/// [`minimal_image`] selects (which honours that ambient pair), the one its stamp names,
/// and no other invocation is given one.
const LAUNCHER_ENV: &[&str] = &["GINUX_ROOT", "GINUX_ROOTFS", "RECIPE_FRAME_TIMEOUT_SECS"];
/// The worker frame deadline OS builds give Recipe when the caller sets none.
const OS_BUILD_FRAME_TIMEOUT_SECS: &str = "1800";

/// Where the pinned compiler image lives, and the file that pins it.
const COMPILER_IMAGE_DIR: &str = ".artifacts/ginux-env/prepared-image-build-tools";
const COMPILER_IMAGE_PIN: &str = "tools/recipe/bootstrap/product-ci.json";

/// Where the pinned minimal runtime image lives, and the file that pins it
/// (`tools/recipe/bootstrap/minimal-runtime.json`, whose `image_manifest` is
/// `ginux-minimal-image.json`). The installed Recipe ships it beside the
/// compiler image; a recipe with `NEEDS.runtime: "minimal"` runs in it, in OS
/// builds too ([`run_images`]).
const MINIMAL_IMAGE_DIR: &str = ".artifacts/ginux-env/prepared-image-minimal";
const MINIMAL_IMAGE_PIN: &str = "tools/recipe/bootstrap/minimal-runtime.json";

/// A pinned image: `name` in its reference (`oci:<name>@sha256:<digest>`),
/// the directory it is produced in and the file whose `image_sha256` pins it.
/// `what` names it in messages and `guide` says how it is produced.
struct PinnedImage {
    what: &'static str,
    name: &'static str,
    dir: &'static str,
    pin: &'static str,
    guide: &'static str,
}

const COMPILER_IMAGE: PinnedImage = PinnedImage {
    what: "compiler image",
    name: "recipe-compiler",
    dir: COMPILER_IMAGE_DIR,
    pin: COMPILER_IMAGE_PIN,
    guide: "tools/recipe/docs/setup-linux.md",
};
const MINIMAL_IMAGE: PinnedImage = PinnedImage {
    what: "minimal runtime image",
    name: "recipe-minimal",
    dir: MINIMAL_IMAGE_DIR,
    pin: MINIMAL_IMAGE_PIN,
    guide: "tools/recipe/bootstrap/README.md (\"Minimal runtime image\")",
};

impl PinnedImage {
    /// The pinned digest, `image_sha256` of the pin file.
    fn digest(&self, repo: &Path) -> Result<String> {
        let pin_file = repo.join(self.pin);
        let pin: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&pin_file).with_context(|| format!("reading '{}'", pin_file.display()))?,
        )
        .with_context(|| format!("parsing '{}'", pin_file.display()))?;
        let digest = pin["image_sha256"]
            .as_str()
            .with_context(|| format!("'{}' has no image_sha256", pin_file.display()))?;
        if digest.is_empty() {
            bail!(
                "'{}' pins no {} yet: produce it as {} describes and set image_sha256 to the sha256 \
                 its provenance.json records",
                pin_file.display(),
                self.what,
                self.guide
            );
        }
        Ok(digest.to_string())
    }

    /// The pinned reference, `oci:<name>@sha256:<image_sha256>`.
    fn reference(&self, repo: &Path) -> Result<String> {
        Ok(format!("oci:{}@sha256:{}", self.name, self.digest(repo)?))
    }

    /// The pinned image as `(reference, rootfs tar)`, produced at
    /// `<dir>/rootfs.tar`, after checking its `provenance.json` carries the
    /// pinned `sha256`. The builder never produces the image.
    fn select(&self, repo: &Path) -> Result<(String, PathBuf)> {
        let digest = self.digest(repo)?;
        let dir = repo.join(self.dir);
        let (rootfs, provenance) = (dir.join("rootfs.tar"), dir.join("provenance.json"));
        let missing = |why: String| {
            anyhow::anyhow!(
                "the {} oci:{}@sha256:{digest} (pinned in {}) is {why}; \
                 produce it as {} describes (expected {} and {})",
                self.what,
                self.name,
                self.pin,
                self.guide,
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
        Ok((format!("oci:{}@sha256:{digest}", self.name), rootfs))
    }
}

/// The compiler image OS builds run Recipe in, as `(GINUX_ROOT, GINUX_ROOTFS)`:
/// the one `product-ci.json` pins (`oci:recipe-compiler@sha256:<image_sha256>`),
/// produced at `.artifacts/ginux-env/prepared-image-build-tools/rootfs.tar`.
/// `None` when the builder's environment sets `GINUX_ROOT` and `GINUX_ROOTFS`
/// ([`ambient_image`]): those keep winning. The builder never produces the image.
fn compiler_image(repo: &Path) -> Result<Option<(String, PathBuf)>> {
    if ambient_image(levitate_recipe::runtime::Image::Full)?.is_some() {
        return Ok(None);
    }
    COMPILER_IMAGE.select(repo).map(Some)
}

/// The image the builder's environment selects for `image`, as `(reference,
/// rootfs tar)`: `GINUX_ROOT` and `GINUX_ROOTFS` for the full image,
/// `GINUX_MINIMAL_ROOT` and `GINUX_MINIMAL_ROOTFS` for the minimal one. Both
/// of a pair or neither: one alone is refused, since Recipe would then take
/// the other half from the host's install, and a key would name another
/// image than the one a run uses and an install ships.
fn ambient_image(image: levitate_recipe::runtime::Image) -> Result<Option<(String, PathBuf)>> {
    let (root, rootfs) = (image.root_env(), image.rootfs_env());
    match (std::env::var_os(root), std::env::var_os(rootfs)) {
        (None, None) => Ok(None),
        (Some(reference), Some(tar)) => {
            let reference = reference
                .into_string()
                .map_err(|_| anyhow::anyhow!("{root} is not valid UTF-8"))?;
            levitate_recipe::runtime::root_digest(&reference)
                .with_context(|| format!("{root}={reference} is not an image reference"))?;
            if tar.is_empty() {
                bail!("{rootfs} is empty; set it to the tar of {root}={reference}");
            }
            Ok(Some((reference, PathBuf::from(tar))))
        }
        _ => bail!(
            "{root} and {rootfs} select an image together: set both (the reference and its tar) \
             or neither (the pinned image)"
        ),
    }
}

/// The minimal runtime image, as `(reference, rootfs tar)`: the ambient
/// `GINUX_MINIMAL_ROOT`/`GINUX_MINIMAL_ROOTFS` pair when the builder's
/// environment sets it ([`ambient_image`]), else the one `minimal-runtime.json`
/// pins (`oci:recipe-minimal@sha256:<digest>`). The installed Recipe ships it,
/// and every builder run whose recipes ask for it runs in it ([`run_images`]).
pub(crate) fn minimal_image(repo: &Path) -> Result<(String, PathBuf)> {
    if let Some(selected) = ambient_image(levitate_recipe::runtime::Image::Minimal)? {
        return Ok(selected);
    }
    MINIMAL_IMAGE.select(repo)
}

/// The minimal image's reference, for keys computed without the image: the
/// ambient `GINUX_MINIMAL_ROOT` when its pair is set, else the pinned one. The
/// same string `minimal_image` selects.
pub(crate) fn minimal_image_reference(repo: &Path) -> Result<String> {
    if let Some((reference, _)) = ambient_image(levitate_recipe::runtime::Image::Minimal)? {
        return Ok(reference);
    }
    pinned_minimal_image_reference(repo)
}

/// The compiler image's reference, for keys computed without the image (CI): the
/// ambient `GINUX_ROOT` when its pair is set, else
/// `oci:recipe-compiler@sha256:<image_sha256>` from `product-ci.json`. The same
/// string `compiler_image` selects.
pub(crate) fn compiler_image_reference(repo: &Path) -> Result<String> {
    if let Some((reference, _)) = ambient_image(levitate_recipe::runtime::Image::Full)? {
        return Ok(reference);
    }
    pinned_compiler_image_reference(repo)
}

/// Which runtime images a builder run of a recipe unpacks: the full
/// (compiler) image for each recipe of its graph that asks for nothing, the
/// minimal image for each that declares `NEEDS.runtime: "minimal"`. Keys of
/// what a run makes name exactly these images ([`RunImages::key_fields`],
/// the done stamp in [`Invocation::run`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct RunImages {
    pub full: bool,
    pub minimal: bool,
}

impl RunImages {
    /// The images runs of all of `recipes` (os-recipes, each with its graph) unpack.
    pub(crate) fn of(repo: &Path, recipes: &[&Path]) -> Result<Self> {
        let mut images = Self::default();
        for recipe in recipes {
            let more = run_images(recipe, &repo.join(OS_RECIPES))?;
            images.full |= more.full;
            images.minimal |= more.minimal;
        }
        Ok(images)
    }

    /// The key fields naming these images, computed without them: `full`
    /// with [`compiler_image_reference`] when a run uses the full image,
    /// `minimal` with [`minimal_image_reference`] when one uses the minimal
    /// image. A key without a run in an image does not change with it.
    pub(crate) fn key_fields(
        self,
        repo: &Path,
        full: &'static str,
        minimal: &'static str,
    ) -> Result<Vec<(&'static str, String)>> {
        let mut fields = Vec::new();
        if self.full {
            fields.push((full, compiler_image_reference(repo)?));
        }
        if self.minimal {
            fields.push((minimal, minimal_image_reference(repo)?));
        }
        Ok(fields)
    }

    /// One reference naming these images, for a store key's root field:
    /// [`compiler_image_reference`] alone for the full image (as such keys
    /// always named it), [`minimal_image_reference`] alone for the minimal
    /// one, both joined by ` + ` when a graph uses both.
    pub(crate) fn reference(self, repo: &Path) -> Result<String> {
        let fields = self.key_fields(repo, "full", "minimal")?;
        Ok(fields.into_iter().map(|(_, reference)| reference).collect::<Vec<_>>().join(" + "))
    }
}

/// The images a run of `recipe`, with names and `deps` resolved from
/// `search`, unpacks: each recipe of its graph by its `NEEDS.runtime`.
pub(crate) fn run_images(recipe: &Path, search: &Path) -> Result<RunImages> {
    let graph = levitate_recipe::needs::declarations(recipe, Some(search))
        .with_context(|| format!("reading the runtime images of '{}'", recipe.display()))?;
    let mut images = RunImages::default();
    for declaration in &graph {
        match declaration.needs.runtime {
            levitate_recipe::runtime::Image::Full => images.full = true,
            levitate_recipe::runtime::Image::Minimal => images.minimal = true,
        }
    }
    Ok(images)
}

/// The builder's recipe collection, relative to the repository.
const OS_RECIPES: &str = "distro/builder/os-recipes";

/// The pinned compiler image's reference, `oci:recipe-compiler@sha256:<image_sha256>`
/// from `product-ci.json`, whatever the environment says.
pub(crate) fn pinned_compiler_image_reference(repo: &Path) -> Result<String> {
    COMPILER_IMAGE.reference(repo)
}

/// The pinned minimal image's reference, `oci:recipe-minimal@sha256:<image_sha256>`
/// from `minimal-runtime.json`, whatever the environment says.
pub(crate) fn pinned_minimal_image_reference(repo: &Path) -> Result<String> {
    MINIMAL_IMAGE.reference(repo)
}

/// Whether `release build --exact` selected exact identity for this process.
static EXACT_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Selects exact identity for the rest of this process (`release build
/// --exact`); call it before the first part key or Recipe invocation.
pub fn select_exact_identity() {
    EXACT_FLAG.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Whether this build uses exact identity (`--exact` selected it). Then every
/// Recipe invocation gets
/// `--exact-identity` (Recipe reuses saved state only from byte-identical
/// Recipe and Ginux binaries), and the system part's key carries the Recipe
/// and launcher bytes (`crate::parts`). Otherwise both name the tools by
/// Recipe's declared behaviour versions (`levitate_recipe::identity`).
pub fn exact_identity() -> bool {
    EXACT_FLAG.load(std::sync::atomic::Ordering::SeqCst)
}

/// The root image Recipe runs in, as `(reference, rootfs tar)`: the ambient
/// `GINUX_ROOT`/`GINUX_ROOTFS` pair when the builder's environment sets it
/// ([`ambient_image`]), else the pinned compiler image `compiler_image`
/// selects. The same image the builder's Recipe runs use, and the one the
/// installed Recipe ships.
pub(crate) fn runtime_image(repo: &Path) -> Result<(String, PathBuf)> {
    if let Some(selected) = ambient_image(levitate_recipe::runtime::Image::Full)? {
        return Ok(selected);
    }
    COMPILER_IMAGE.select(repo)
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
    crate::teprintln!(
        "  building ginux (cargo build -p ginux-init --bin ginux --release in {GINUX_DIR})"
    );
    let step = crate::timeline::Step::begin("tool", "cargo build ginux");
    Cmd::new("cargo")
        .args([
            "build",
            "-q",
            "-p",
            "ginux-init",
            "--bin",
            "ginux",
            "--release",
            "--target",
            GINUX_TARGET,
        ])
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

/// One run of a recipe command (`install`, `cleanup`) in its work directory;
/// imports resolve from `<repo>/distro/builder/os-recipes`.
#[derive(Clone)]
pub struct Invocation<'a> {
    repo: &'a Path,
    phase: &'static str,
    recipe: PathBuf,
    work: PathBuf,
    args: Vec<OsString>,
    root: Option<(String, PathBuf)>,
    worker: Option<PathBuf>,
    cwd: Option<PathBuf>,
    /// Runs beside other invocations: its own progress file, no timeline step.
    concurrent: Option<PathBuf>,
    /// Where the run's output goes instead of the terminal.
    log: Option<PathBuf>,
    /// The recipe collection names and deps resolve from, when not os-recipes.
    recipes_path: Option<PathBuf>,
    /// Environment the builder sets for this run (after removing ambient
    /// `RECIPE_*`/`GINUX_*` variables).
    env: Vec<(OsString, OsString)>,
    /// Clean the program up (`recipe cleanup`) before the run, so every step
    /// runs again, and never reuse a done stamp.
    redo: bool,
    /// Keep only this run's result of its stamp slot (`discard_superseded`).
    discard_superseded: bool,
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
            concurrent: None,
            log: None,
            recipes_path: None,
            env: Vec::new(),
            redo: false,
            discard_superseded: false,
        }
    }

    /// Resolves recipe names and `deps` from `path` instead of os-recipes: a
    /// group recipe of the public catalogue (`recipes/`) run by the builder.
    pub fn recipes_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.recipes_path = Some(path.into());
        self
    }

    /// Sets `name` to `value` in Recipe's environment for this run, such as
    /// `RECIPE_HOST_LIBC_ROOT` (the system a group recipe is built for).
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((name.into(), value.into()));
        self
    }

    /// Runs beside other invocations of the same build (the unpack shards):
    /// Recipe keeps its progress in `progress_file`, and the run records no
    /// timeline step and no active recipe of its own, since the timeline is
    /// one stack; the caller records the step they make up together.
    pub fn concurrent(mut self, progress_file: impl Into<PathBuf>) -> Self {
        self.concurrent = Some(progress_file.into());
        self
    }

    /// Sends what the run prints to `log` instead of the terminal.
    pub fn log_file(mut self, log: impl Into<PathBuf>) -> Self {
        self.log = Some(log.into());
        self
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

    /// Run every step again, even when Recipe holds downloads or a result
    /// already: `recipe cleanup NAME` first, then the run (specification 6.3,
    /// decision 20). Producer runs otherwise follow Recipe's disk rules
    /// (decision 23); the builder keeps its own stamps and passes no `--keep`.
    pub fn redo(mut self, redo: bool) -> Self {
        self.redo = redo;
        self
    }

    /// For a caller that reads the result only while the builder runs
    /// (copies it, or passes it to a run that does): once this run's result
    /// is saved or reused, the earlier results of its stamp slot (the same
    /// recipe, work directory and flags, other input contents) are
    /// discarded with their publications in Recipe's exports, which nothing
    /// else would ever delete (`stamp`). Also in exact mode, which reuses no
    /// stamp but records one so the next run can discard its publication.
    /// Never for a result something keeps a path to beyond this process
    /// (the kernel bundle `current` links to, the recorded preseed outputs).
    pub fn discard_superseded(mut self) -> Self {
        self.discard_superseded = true;
        self
    }

    pub fn run(self) -> Result<RecipeResult> {
        if self.redo {
            let mut cleanup = self.clone();
            cleanup.phase = "cleanup";
            cleanup.redo = false;
            cleanup.run()?;
        }
        let recipe = &self.recipe;
        let work = &self.work;
        let recipe_name = recipe
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| recipe.display().to_string());
        // The stamp reads the recipe file as Recipe will find it: a path
        // relative to the run's folder, else a bare name from os-recipes
        // (`tui-binary`), which Recipe resolves through --recipes-path.
        let run_dir = self.cwd.as_deref().unwrap_or(self.repo);
        let recipe_file = if recipe.is_absolute() || run_dir.join(recipe).is_file() {
            run_dir.join(recipe)
        } else {
            let named = self
                .repo
                .join(OS_RECIPES)
                .join(recipe)
                .with_extension("rhai");
            if named.is_file() {
                named
            } else {
                run_dir.join(recipe)
            }
        };
        // OS producers and their imports use the builder's collection;
        // the workspace's user-managed recipe catalog is a separate consumer.
        let os_recipes_path = self
            .recipes_path
            .clone()
            .unwrap_or_else(|| self.repo.join(OS_RECIPES));
        // The images the run unpacks (`run_images`). A run in its own root
        // has that root as both images, so a recipe asking for the minimal
        // one runs in it too.
        let images = if self.root.is_some() {
            RunImages { full: true, minimal: false }
        } else {
            run_images(&recipe_file, &os_recipes_path)?
        };
        // The full image: this run's own root, else the pinned compiler image;
        // None when the ambient GINUX_ROOT/GINUX_ROOTFS pair names it, which
        // reaches Recipe as it is (LAUNCHER_ENV).
        let full_image = match &self.root {
            Some(own) => Some(own.clone()),
            None => compiler_image(self.repo)?,
        };
        let full_reference = match &full_image {
            Some((reference, _)) => reference.clone(),
            None => compiler_image_reference(self.repo)?,
        };
        // The minimal image, only for a run whose recipes ask for it.
        let minimal_image = if self.root.is_none() && images.minimal {
            Some(minimal_image(self.repo)?)
        } else {
            None
        };
        // An install whose recipe, inputs, images and Recipe behaviour are
        // unchanged and whose outputs are intact is not run again (`stamp`).
        // Its root field names the images the run unpacks and no other: the
        // compiler image's reference alone for a run in it (as before the
        // minimal image), the minimal image's alone, or both.
        let root_reference = match (images.full, &minimal_image) {
            (_, None) => full_reference,
            (false, Some((minimal, _))) => minimal.clone(),
            (true, Some((minimal, _))) => format!("{full_reference} + {minimal}"),
        };
        let stamp = if self.redo || (exact_identity() && !self.discard_superseded) {
            None
        } else {
            stamp::Stamp::for_run(
                self.repo,
                self.phase,
                &recipe_file,
                work,
                &self.args,
                &root_reference,
                self.worker.as_deref(),
            )
            .with_context(|| format!("computing the done stamp of {}", recipe.display()))?
        };
        let reused = stamp
            .as_ref()
            .filter(|_| !exact_identity())
            .and_then(stamp::Stamp::reuse);
        if let Some(result) = reused {
            self.discard_superseded_by(stamp.as_ref(), &result, None);
            let step = self.concurrent.is_none().then(|| {
                crate::timeline::Step::begin(
                    "recipe",
                    &format!("recipe {} {recipe_name} (unchanged)", self.phase),
                )
            });
            crate::tprintln!(
                "  recipe {} {}: unchanged since its last run (stamp {}); reusing its result",
                self.phase,
                recipe.display(),
                stamp.as_ref().map(stamp::Stamp::short).unwrap_or_default()
            );
            if let Some(step) = step {
                step.finish();
            }
            return Ok(result);
        }
        let step = self.concurrent.is_none().then(|| {
            crate::timeline::Step::begin("recipe", &format!("recipe {} {recipe_name}", self.phase))
        });
        // What the builder says about the run: on the terminal, or at the top
        // of its log.
        let mut log = match &self.log {
            Some(path) => Some(
                std::fs::File::create(path)
                    .with_context(|| format!("creating the recipe log '{}'", path.display()))?,
            ),
            None => None,
        };
        let mut note = |line: String| match log.as_mut() {
            Some(file) => {
                use std::io::Write as _;
                let _ = writeln!(file, "{} {line}", crate::timeline::clock());
            }
            None => crate::teprintln!("{line}"),
        };
        note(format!(
            "  recipe {} {} (work: {})",
            self.phase,
            recipe.display(),
            work.display()
        ));
        let bin = binary(self.repo)?;
        std::fs::create_dir_all(work)
            .with_context(|| format!("creating recipe work directory '{}'", work.display()))?;
        // This invocation must write its own result: never read a stale one.
        let result = tempfile::Builder::new()
            .prefix(".recipe-result.")
            .suffix(".json")
            .tempfile_in(work)
            .context("creating a fresh recipe result file")?;

        let mut cmd = Cmd::new(bin)
            // OS composition consumes products without activating host packages.
            .arg("--no-activate")
            // OS producers always run inside Ginux, in the pinned image, even
            // those that start no program and would run host-side under auto.
            .arg("--execution")
            .arg("ginux")
            .arg(self.phase)
            .arg(recipe)
            .arg("--build-dir")
            .arg(work)
            .arg("--state-dir")
            .arg(state_dir(work))
            .arg("--json-output")
            .arg(result.path())
            .arg("--recipes-path")
            .arg(&os_recipes_path)
            .args(exact_identity().then_some("--exact-identity"))
            .args(&self.args)
            .dir(self.cwd.as_deref().unwrap_or(self.repo))
            .stdout_to_stderr()
            .error_msg(format!("recipe {} {}", self.phase, recipe.display()));
        // Recipe keeps this file current (done/total of what it is doing); the
        // builder records its path, so `cargo xtask progress` can read it.
        let progress = (self.phase == "install").then(|| {
            self.concurrent
                .clone()
                .unwrap_or_else(|| crate::timeline::progress_file(self.repo, &recipe_name))
        });
        let mut _active = None;
        if let Some(file) = &progress {
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            cmd = cmd.arg("--progress-file").arg(file);
            if self.concurrent.is_none() {
                crate::timeline::recipe_begin(&recipe_name, file);
                _active = Some(ActiveRecipe);
            }
        }
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            let control = name.starts_with("RECIPE_") || name.starts_with("GINUX_");
            if control && !LAUNCHER_ENV.contains(&name.as_ref()) {
                cmd = cmd.env_remove(&key);
            }
        }
        // Pinned downloads are read from the builder's download store before
        // the network (Recipe's offline cache: `<dir>/sha256/<digest>`, read
        // only to Recipe). A superseded upstream file, such as a replaced
        // Fedora update build, then does not fail a run whose pin the store
        // already holds. A caller's own `.env("RECIPE_CACHE_DIR", ..)` below wins.
        if let Ok(store) = rootfs_source::store_dir(self.repo)
            .parent()
            .map_or_else(|| Err(std::io::ErrorKind::NotFound.into()), std::fs::canonicalize)
        {
            if store.join("sha256").is_dir() {
                static SHARED: std::sync::Once = std::sync::Once::new();
                SHARED.call_once(|| {
                    if let Err(err) = rootfs_source::share_store(&store.join("sha256")) {
                        crate::teprintln!(
                            "warning: Recipe runs cannot read the whole download store: {err:#}"
                        );
                    }
                });
                note(format!("    RECIPE_CACHE_DIR={} (the download store)", store.display()));
                cmd = cmd.env("RECIPE_CACHE_DIR", &store);
            }
        }
        for (name, value) in &self.env {
            note(format!(
                "    {}={}",
                name.to_string_lossy(),
                value.to_string_lossy()
            ));
            cmd = cmd.env(name, value);
        }
        let ginux = ginux_binary(self.repo)?;
        let worker = self.worker.as_deref().unwrap_or(bin);
        cmd = cmd.env("GINUX_BIN", ginux).env("RECIPE_WORKER_BIN", worker);
        for (name, value) in [("GINUX_BIN", ginux), ("RECIPE_WORKER_BIN", worker)] {
            if let Some(ambient) =
                std::env::var_os(name).filter(|ambient| Path::new(ambient) != value)
            {
                note(format!(
                    "    {name}={} (the builder's own; ignoring {})",
                    value.display(),
                    Path::new(&ambient).display()
                ));
            } else {
                note(format!("    {name}={}", value.display()));
            }
        }
        // Recipe allows a worker frame 300 s by default, and a checkpoint is one
        // frame: snapshotting a step's storage, several GB for a desktop
        // tree, outlasted that under a busy disk and failed package effects
        // on 2026-11-12. Unless the caller set one, OS builds allow 30 min.
        if std::env::var_os("RECIPE_FRAME_TIMEOUT_SECS").is_none() {
            cmd = cmd.env("RECIPE_FRAME_TIMEOUT_SECS", OS_BUILD_FRAME_TIMEOUT_SECS);
        }
        for name in LAUNCHER_ENV {
            // This invocation's own root replaces the ambient one.
            if self.root.is_some() && name.starts_with("GINUX_ROOT") {
                continue;
            }
            if let Some(value) = std::env::var_os(name) {
                note(format!("    {name}={}", Path::new(&value).display()));
            }
        }
        if let Some((reference, rootfs)) = &full_image {
            cmd = cmd.env("GINUX_ROOT", reference).env("GINUX_ROOTFS", rootfs);
            let what = if self.root.is_some() { "this recipe's own root" } else { "the pinned compiler image" };
            note(format!("    GINUX_ROOT={reference} ({what})"));
            note(format!("    GINUX_ROOTFS={}", rootfs.display()));
        }
        // A run in its own root has it as the minimal image too; another run
        // gets a minimal image only when its recipes ask for one, the one its
        // stamp names (ambient ones were removed above).
        let minimal = match &self.root {
            Some(own) => Some((own.clone(), "this recipe's own root")),
            None => minimal_image.clone().map(|image| (image, "the minimal runtime image")),
        };
        if let Some(((reference, rootfs), what)) = minimal {
            cmd = cmd
                .env("GINUX_MINIMAL_ROOT", &reference)
                .env("GINUX_MINIMAL_ROOTFS", &rootfs);
            if self.root.is_none() {
                note(format!("    GINUX_MINIMAL_ROOT={reference} ({what})"));
                note(format!("    GINUX_MINIMAL_ROOTFS={}", rootfs.display()));
            }
        }
        if let Some(file) = log.take() {
            cmd = cmd
                .output_to(file)
                .context("sending the recipe's output to its log")?;
        }
        cmd.run_interactive()?;

        let bytes =
            std::fs::read(result.path()).context("reading this invocation's recipe JSON result")?;
        let result = RecipeResult::from_json(&bytes)?;
        if let Some(stamp) = &stamp {
            let previous = self.discard_superseded.then(|| stamp.previous()).flatten();
            match stamp.save(&result) {
                Ok(()) => self.discard_superseded_by(Some(stamp), &result, previous),
                Err(error) => crate::teprintln!("  (no done stamp for {}: {error:#})", recipe.display()),
            }
        }
        if let Some(file) = &progress {
            let _ = std::fs::remove_file(file);
        }
        if let Some(step) = step {
            step.finish();
        }
        Ok(result)
    }
}

impl Invocation<'_> {
    /// With `discard_superseded`, discards the other results of `stamp`'s
    /// slot (`stamp::Stamp::discard_superseded`); a failure is a warning,
    /// never the run's.
    fn discard_superseded_by(
        &self,
        stamp: Option<&stamp::Stamp>,
        result: &RecipeResult,
        previous: Option<(std::fs::File, serde_json::Value)>,
    ) {
        let Some(stamp) = stamp.filter(|_| self.discard_superseded) else {
            return;
        };
        match stamp.discard_superseded(&state_dir(&self.work), result, previous) {
            Ok(0) => {}
            Ok(count) => crate::tprintln!(
                "  recipe {} {}: discarded {count} superseded result(s) and their publications",
                self.phase,
                self.recipe.display()
            ),
            Err(error) => crate::teprintln!(
                "  warning: the superseded results of {} were not discarded: {error:#}",
                self.recipe.display()
            ),
        }
    }
}

/// The Recipe state directory of a run in the work directory `work`.
fn state_dir(work: &Path) -> PathBuf {
    work.join("recipe-state")
}

/// Clears the recorded Recipe invocation when the run ends, however it ends.
struct ActiveRecipe;

impl Drop for ActiveRecipe {
    fn drop(&mut self) {
        crate::timeline::recipe_end();
    }
}
