//! The kernel of a variant, owned here: its pin (read from the kernel recipe),
//! the bundle layout, its kconfig, and the one Recipe invocation that installs
//! or checks it. xtask and the release build call these functions.

use anyhow::{bail, Context, Result};
use distro_contract::Variant;
use std::path::{Path, PathBuf};

use crate::recipe::{recipe_work_dir, Invocation, RecipeResult};
use distro_contract::runs::kernel_dir;

/// The kernel recipes, relative to the repo root. Autofix may edit only these.
const RECIPES: &str = "distro/builder/recipes";
/// The pins and the bundle rules both kernel recipes import.
const METADATA: &str = "kernel/metadata.rhai";
/// The kernel configuration every variant shares; the variant's own `kconfig`
/// (options of its kernel line only) is applied after it.
const SHARED_KCONFIG: &str = "distro/variants/_shared/kconfig";

/// `kernel.release` under the kernel output directory.
pub const KERNEL_RELEASE_PATH: &str = "kernel-build/include/config/kernel.release";
/// The kernel image under the kernel output directory.
pub const KERNEL_IMAGE_PATH: &str = "staging/boot/vmlinuz";
/// The directory under the kernel output directory that holds `<kernel.release>/`
/// with the modules (`/usr/lib/modules` in the image).
pub const KERNEL_MODULES_DIR: &str = "staging/usr/lib/modules";

/// A variant's kernel as `kernel/metadata.rhai` pins it.
#[derive(Debug, Clone)]
pub struct KernelPin {
    /// The kernel.org version, e.g. `6.12.116`.
    pub version: String,
    /// `-<variant id>`.
    pub localversion: String,
    /// The exact `kernel.release` the pin builds: version and localversion.
    pub release: String,
    /// The sha256 of the version's kernel.org source tarball.
    pub sha256: String,
}

/// Where `install` gets the kernel.
#[derive(Debug, Clone)]
pub enum Source {
    /// The published prebuilt bundle (`linux-prebuilt.rhai`).
    Prebuilt,
    /// A source build (`linux.rhai`), optionally letting Recipe's autofix
    /// patch the kernel recipes after a failure.
    Compile { autofix: Option<Autofix> },
}

#[derive(Debug, Clone)]
pub struct Autofix {
    pub attempts: u8,
    pub prompt_file: Option<PathBuf>,
    pub llm_profile: Option<String>,
}

/// What `.artifacts/kernel/<id>/current` holds.
#[derive(Debug, Clone)]
pub enum KernelStatus {
    /// The pinned kernel; its release.
    Ready(String),
    /// No kernel, and why.
    Missing(String),
    /// A kernel that is not the pinned one, and why.
    Mismatch(String),
}

/// The kernel recipes' Rhai: the pin table and the bundle rules, evaluated with
/// the few Recipe helpers they use (Recipe itself is not needed for these).
struct Metadata {
    engine: rhai::Engine,
    ast: rhai::AST,
}

impl Metadata {
    fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(RECIPES).join(METADATA);
        let mut engine = rhai::Engine::new();
        engine
            .register_fn("join_path", |a: &str, b: &str| {
                Path::new(a).join(b).to_string_lossy().into_owned()
            })
            .register_fn("is_file", |p: &str| Path::new(p).is_file())
            .register_fn("is_dir", |p: &str| Path::new(p).is_dir())
            .register_fn("read_file_or_empty", |p: &str| {
                std::fs::read_to_string(p).unwrap_or_default()
            })
            .register_fn("trim", |s: &str| s.trim().to_string());
        let ast = engine
            .compile_file(path.clone())
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("reading the kernel pins in {}", path.display()))?;
        Ok(Self { engine, ast })
    }

    fn call<T: rhai::Variant + Clone>(&self, name: &str, args: impl rhai::FuncArgs) -> Result<T> {
        self.engine
            .call_fn::<T>(&mut rhai::Scope::new(), &self.ast, name, args)
            .map_err(|e| anyhow::anyhow!("{METADATA} {name}: {e}"))
    }

    /// The variant's pin, as the map the recipes use and as a [`KernelPin`].
    fn spec(&self, variant: &Variant) -> Result<(rhai::Map, KernelPin)> {
        let spec: rhai::Map = self.call("source_spec", (variant.id.clone(),))?;
        let field = |name: &str| -> Result<String> {
            spec.get(name)
                .and_then(|v| v.clone().into_string().ok())
                .with_context(|| format!("{METADATA} source_spec has no string '{name}'"))
        };
        let line = field("line")?;
        let declared = variant.contract.profile.kernel_line.as_str();
        if line != declared {
            bail!(
                "{METADATA} pins {} to the '{line}' kernel line, but its distro.toml [profile] kernel_line is '{declared}'",
                variant.id
            );
        }
        let pin = KernelPin {
            version: field("version")?,
            localversion: field("localversion")?,
            release: field("release")?,
            sha256: field("sha256")?,
        };
        Ok((spec, pin))
    }
}

/// The variant's kernel pin.
pub fn pin(variant: &Variant) -> Result<KernelPin> {
    Ok(Metadata::load(&variant.repo_root)?.spec(variant)?.1)
}

/// What the variant's kernel output directory holds, by the kernel recipes'
/// own bundle rule. Reads files only: no Recipe run, no privileges.
pub fn status(variant: &Variant) -> Result<KernelStatus> {
    let metadata = Metadata::load(&variant.repo_root)?;
    let (spec, pin) = metadata.spec(variant)?;
    let current = kernel_dir(&variant.repo_root, &variant.id);
    if std::fs::symlink_metadata(&current).is_err() {
        return Ok(KernelStatus::Missing(format!("{} does not exist", current.display())));
    }
    let root = bundle_root(&current)?;
    let root = root.to_str().context("the kernel output path is not UTF-8")?.to_owned();
    let mismatch: String = metadata.call("bundle_mismatch", (root, spec))?;
    Ok(if mismatch.is_empty() {
        KernelStatus::Ready(pin.release)
    } else if !current.join(KERNEL_RELEASE_PATH).is_file() {
        KernelStatus::Missing(mismatch)
    } else {
        KernelStatus::Mismatch(mismatch)
    })
}

/// Install the variant's kernel from `source` (again, with `force`) and select
/// it as its kernel output directory. Returns the installed release.
pub fn install(variant: &Variant, source: &Source, force: bool) -> Result<String> {
    let repo_root = &variant.repo_root;
    let current = kernel_dir(repo_root, &variant.id);
    let kconfig = merged_kconfig(variant)?;
    let target = existing_bundle(&current)?;
    let mut inputs: Vec<(&str, &Path)> = vec![("kconfig", &kconfig)];
    if let Some(target) = &target {
        inputs.push(("target", target));
    }
    let mut args: Vec<String> = Vec::new();
    let recipe = match source {
        Source::Prebuilt => {
            if force {
                args.extend(["--force-acquire".into(), "--force-install".into()]);
            }
            // GitHub redirects release downloads to its asset host.
            "linux-prebuilt.rhai"
        }
        Source::Compile { autofix } => {
            if force {
                args.extend(["--force-build".into(), "--force-install".into()]);
            }
            if let Some(fix) = autofix {
                args.extend(["--autofix".into(), "--autofix-attempts".into(), fix.attempts.to_string()]);
                args.extend(["--autofix-cwd".into(), repo_root.display().to_string()]);
                args.extend(["--autofix-allow-path".into(), RECIPES.into()]);
                if let Some(prompt) = &fix.prompt_file {
                    args.extend(["--autofix-prompt-file".into(), prompt.display().to_string()]);
                }
                if let Some(profile) = &fix.llm_profile {
                    args.extend(["--llm-profile".into(), profile.clone()]);
                }
            }
            "linux.rhai"
        }
    };
    let ctx = run(variant, "install", recipe, &inputs, &args)?;
    crate::recipe::linux::select_kernel_bundle(&ctx, &current)?;
    Ok(ctx.metadata_string("kernel_version")?.to_owned())
}

/// Ask the prebuilt kernel recipe whether `kernel_output_dir` holds the
/// variant's kernel (the release build's check: it never builds a kernel).
pub fn check_kernel_installed(variant: &Variant, kernel_output_dir: &Path) -> Result<()> {
    let kconfig = merged_kconfig(variant)?;
    let target = bundle_root(kernel_output_dir)?;
    let inputs: Vec<(&str, &Path)> = vec![("kconfig", &kconfig), ("target", &target)];
    run(variant, "isinstalled", "linux-prebuilt.rhai", &inputs, &[])
        .with_context(|| format!("the {} kernel is not installed", variant.id))?;
    Ok(())
}

fn run(
    variant: &Variant,
    phase: &'static str,
    recipe: &str,
    inputs: &[(&str, &Path)],
    args: &[String],
) -> Result<RecipeResult> {
    let repo_root = &variant.repo_root;
    let mut invocation = Invocation::new(
        repo_root,
        phase,
        repo_root.join(RECIPES).join(recipe),
        recipe_work_dir(repo_root, &variant.id)?,
    )
    .define("KERNEL_TARGET", &variant.id)
    .args(args);
    for (name, path) in inputs {
        invocation = invocation.input(name, path);
    }
    invocation.run()
}

/// The bundle behind the kernel output directory, a symlink to it: Recipe
/// stages only real files and directories.
fn bundle_root(current: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(current)
        .with_context(|| format!("resolving the kernel output {}", current.display()))
}

/// The bundle behind `current`, or `None` when there is no kernel output yet.
fn existing_bundle(current: &Path) -> Result<Option<PathBuf>> {
    if std::fs::symlink_metadata(current).is_err() {
        return Ok(None);
    }
    bundle_root(current).map(Some)
}

/// The kernel configuration of the variant: the shared kconfig, then the
/// variant's own.
pub fn kconfig_text(variant: &Variant) -> Result<String> {
    let mut text = String::new();
    for path in [variant.repo_root.join(SHARED_KCONFIG), variant.path("kconfig")] {
        text += &std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        text.push('\n');
    }
    Ok(text)
}

/// The kconfig the recipes are given: [`kconfig_text`], written to the
/// variant's recipe work directory.
fn merged_kconfig(variant: &Variant) -> Result<PathBuf> {
    let text = kconfig_text(variant)?;
    let work = recipe_work_dir(&variant.repo_root, &variant.id)?;
    std::fs::create_dir_all(&work)
        .with_context(|| format!("creating {}", work.display()))?;
    let path = work.join("kconfig");
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
