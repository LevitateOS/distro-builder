//! The Ginux every builder Recipe run uses: the checkout's five tools, built
//! once per process ([`tools`]), and the root-owned helper wrapper the `ginux`
//! CLI runs through `sudo` ([`helper_wrapper`]).
//!
//! The CLI runs `sudo HELPER --protocol 1 ...`; `sudo` resets the environment,
//! so the helper finds its runtime, image tool and allocator only at the paths
//! the wrapper exports. The helper and everything it starts run as root, so the
//! wrapper and the copies it names live in a root-owned directory nobody else
//! can write ([`WRAPPER_DIR`]); the user-owned `.artifacts` and `target`
//! directories never hold a file root executes through `sudo`. The host's
//! `/usr/libexec` is never touched.

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::process::Cmd;

/// The Ginux checkout, relative to the repository.
const GINUX_DIR: &str = "tools/ginux";
/// The runtime is freestanding: `tools/ginux/.cargo/config.toml` gives this
/// target its static, no-libc link flags.
const RUNTIME_TARGET: &str = "x86_64-unknown-linux-gnu";
/// The CLI and the image tool are static musl programs: the same bytes run on
/// the build host and in both distros' images (AcornOS has no glibc).
const STATIC_TARGET: &str = "x86_64-unknown-linux-musl";
/// The helper is a shell script in the checkout.
const HELPER_SOURCE: &str = "scripts/ginux-host-instance-helper.sh";

/// Where the builder keeps root-owned copies of the helper, runtime, image tool
/// and allocator, and the wrapper Recipe's runs use as `GINUX_INSTANCE_HELPER`.
pub const WRAPPER_DIR: &str = "/var/lib/levitate-builder/ginux";
/// The wrapper's file name in [`WRAPPER_DIR`].
const WRAPPER: &str = "ginux-helper-wrapper";

/// The root-owned copy `name` (`ginux-runtime`, `ginux-image`) in
/// [`WRAPPER_DIR`] that the wrapper exports to the helper. Recipe's runs get
/// the same paths as `GINUX_RUNTIME` and `GINUX_IMAGE_TOOL`, so its exact
/// identity of Ginux (`levitate_recipe::ginux::Ginux::digest`) hashes the
/// files the helper runs.
pub fn tool_copy(name: &str) -> PathBuf {
    Path::new(WRAPPER_DIR).join(name)
}

/// The five Ginux files a Recipe with the public lifecycle uses and an
/// installed system ships.
#[derive(Debug, Clone)]
pub struct GinuxTools {
    /// The `ginux` CLI (`ginux-cli`), which Recipe runs (`RECIPE_GINUX`).
    pub cli: PathBuf,
    /// The privileged host helper (`ginux-helper`).
    pub helper: PathBuf,
    /// The image tool (`ginux-oci`, installed as `ginux-image`).
    pub image: PathBuf,
    /// The runtime (`ginux-init --bin ginux`, installed as `ginux-runtime`).
    pub runtime: PathBuf,
    /// The allocator the helper copies into every instance root.
    pub allocator: PathBuf,
}

impl GinuxTools {
    /// The five files with their installed names, in a fixed order.
    pub fn files(&self) -> [(&'static str, &Path); 5] {
        [
            ("ginux-cli", &self.cli),
            ("ginux-helper", &self.helper),
            ("ginux-image", &self.image),
            ("ginux-runtime", &self.runtime),
            ("libginux_allocator.so", &self.allocator),
        ]
    }

    /// One digest of the five files' contents: SHA-256 over each installed
    /// name and that file's SHA-256, in [`Self::files`] order.
    pub fn digest(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        for (name, path) in self.files() {
            hasher.update(name.as_bytes());
            hasher.update([0]);
            hasher.update(crate::parts::file_sha256(path)?.0.as_bytes());
            hasher.update([0]);
        }
        Ok(hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }
}

/// The checkout's Ginux tools, built (release) on first use in each process,
/// so a run never uses older Ginux than the checkout:
/// - the runtime: `cargo build -p ginux-init --bin ginux --target x86_64-unknown-linux-gnu`;
/// - the CLI and image tool: `cargo build -p ginux-cli -p ginux-oci --target x86_64-unknown-linux-musl`
///   with `RUSTFLAGS` empty (the checkout's target flags are the runtime's);
/// - the allocator: `cargo build -p ginux-allocator` for the host, `RUSTFLAGS` empty;
/// - the helper: `tools/ginux/scripts/ginux-host-instance-helper.sh` as it is.
pub fn tools(repo: &Path) -> Result<&'static GinuxTools> {
    static TOOLS: OnceLock<GinuxTools> = OnceLock::new();
    if let Some(tools) = TOOLS.get() {
        return Ok(tools);
    }
    let dir = repo.join(GINUX_DIR);
    // Beside the builder's own builds when they have a target directory of their
    // own (a build run as root keeps root's files out of tools/ginux).
    let target_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(|target| PathBuf::from(target).join("ginux"))
        .unwrap_or_else(|| dir.join("target"));
    // The ordinary programs build apart from the freestanding runtime, so
    // their different flags never make cargo rebuild the other's crates.
    let host_dir = target_dir.join("host");
    let step = crate::timeline::Step::begin("tool", "cargo build ginux");
    let cargo = |what: &str, args: &[&str], target: &Path, plain_flags: bool| -> Result<()> {
        crate::teprintln!(
            "  building {what} (cargo build {} in {GINUX_DIR})",
            args.join(" ")
        );
        let mut cmd = Cmd::new("cargo")
            .args(["build", "-q", "--release"])
            .args(args)
            .arg("--target-dir")
            .arg(target)
            .dir(&dir)
            .stdout_to_stderr()
            .error_msg(format!("building {what}"));
        if plain_flags {
            // The C parts of a dependency (ring, for the image tool's TLS)
            // compile with the host's gcc; Rust's musl target links them.
            cmd = cmd
                .env("RUSTFLAGS", "")
                .env("CC_x86_64_unknown_linux_musl", "gcc");
        }
        cmd.run_interactive()?;
        Ok(())
    };
    cargo(
        "the Ginux runtime",
        &[
            "-p",
            "ginux-init",
            "--bin",
            "ginux",
            "--target",
            RUNTIME_TARGET,
        ],
        &target_dir,
        false,
    )?;
    cargo(
        "the ginux CLI and image tool",
        &[
            "-p",
            "ginux-cli",
            "-p",
            "ginux-oci",
            "--target",
            STATIC_TARGET,
        ],
        &host_dir,
        true,
    )?;
    cargo(
        "the Ginux allocator",
        &["-p", "ginux-allocator"],
        &host_dir,
        true,
    )?;
    let tools = GinuxTools {
        cli: host_dir.join(STATIC_TARGET).join("release/ginux-cli"),
        helper: dir.join(HELPER_SOURCE),
        image: host_dir.join(STATIC_TARGET).join("release/ginux-oci"),
        runtime: target_dir.join(RUNTIME_TARGET).join("release/ginux"),
        allocator: host_dir.join("release/libginux_allocator.so"),
    };
    for (name, path) in tools.files() {
        if !path.is_file() {
            bail!(
                "the Ginux build made no {name}: '{}' is missing",
                path.display()
            );
        }
    }
    step.finish();
    Ok(TOOLS.get_or_init(|| tools))
}

/// The root-owned helper wrapper every Recipe run of this process passes as
/// `GINUX_INSTANCE_HELPER`, made on first use: [`WRAPPER_DIR`] (each level a
/// real directory owned by root without group or other write permission,
/// missing levels made 0755) holds copies of the checkout's helper, runtime,
/// image tool and allocator, and the wrapper, which exports `GINUX_RUNTIME`,
/// `GINUX_IMAGE_TOOL` and `GINUX_ALLOCATOR_LIBRARY` for those copies and
/// execs the helper copy. Each file is replaced by a rename, so a helper
/// another build is running keeps its own bytes. Root only: the builder
/// runs as root for OS builds (`cargo xtask build` elevates itself).
pub fn helper_wrapper(repo: &Path) -> Result<&'static Path> {
    static WRAPPER_PATH: OnceLock<PathBuf> = OnceLock::new();
    if let Some(path) = WRAPPER_PATH.get() {
        return Ok(path);
    }
    let tools = tools(repo)?;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } != 0 {
        bail!(
            "Recipe runs in Ginux through the root-only Ginux helper: run the builder as root \
             (`cargo xtask build` runs it through sudo -n)"
        );
    }
    let dir = Path::new(WRAPPER_DIR);
    trusted_dir(dir)?;
    let copies = [
        ("ginux-helper", &tools.helper),
        ("ginux-runtime", &tools.runtime),
        ("ginux-image", &tools.image),
        ("libginux_allocator.so", &tools.allocator),
    ];
    for (name, source) in copies {
        let bytes = fs::read(source).with_context(|| format!("reading '{}'", source.display()))?;
        install(&dir.join(name), &bytes)?;
    }
    let path = |name: &str| tool_copy(name).display().to_string();
    // The helper's digest is written into the wrapper, so the wrapper's bytes
    // (what Recipe hashes as the helper in its exact identity of Ginux) change
    // whenever the helper does.
    let wrapper = format!(
        "#!/bin/sh\n\
         # Written by distro-builder: the checkout's Ginux helper with its tools.\n\
         # helper sha256 {}\n\
         GINUX_RUNTIME='{}'\n\
         GINUX_IMAGE_TOOL='{}'\n\
         GINUX_ALLOCATOR_LIBRARY='{}'\n\
         export GINUX_RUNTIME GINUX_IMAGE_TOOL GINUX_ALLOCATOR_LIBRARY\n\
         exec '{}' \"$@\"\n",
        crate::parts::file_sha256(&tools.helper)?.0,
        path("ginux-runtime"),
        path("ginux-image"),
        path("libginux_allocator.so"),
        path("ginux-helper"),
    );
    let wrapper_path = dir.join(WRAPPER);
    install(&wrapper_path, wrapper.as_bytes())?;
    Ok(WRAPPER_PATH.get_or_init(|| wrapper_path))
}

/// Makes `dir` and every ancestor a real directory owned by root without
/// group or other write permission, creating missing levels 0755.
fn trusted_dir(dir: &Path) -> Result<()> {
    let mut level = PathBuf::from("/");
    for component in dir.components().skip(1) {
        level.push(component);
        match fs::symlink_metadata(&level) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&level)
                    .with_context(|| format!("creating '{}'", level.display()))?;
                fs::set_permissions(&level, fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("setting the mode of '{}'", level.display()))?;
            }
            Err(error) => {
                return Err(error).with_context(|| format!("inspecting '{}'", level.display()))
            }
        }
        let metadata = fs::symlink_metadata(&level)
            .with_context(|| format!("inspecting '{}'", level.display()))?;
        if !metadata.is_dir() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            bail!(
                "'{}' must be a directory owned by root that only root can write: the Ginux helper \
                 runs as root from {WRAPPER_DIR}",
                level.display()
            );
        }
    }
    Ok(())
}

/// Writes `bytes` to `path` (mode 0755, owned by the caller, root) through a
/// temporary file in the same directory and a rename.
fn install(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("an installed Ginux file has no directory")?;
    let mut file = tempfile::Builder::new()
        .prefix(".install-")
        .tempfile_in(parent)
        .with_context(|| format!("creating a file in '{}'", parent.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("writing '{}'", path.display()))?;
    file.as_file()
        .sync_all()
        .with_context(|| format!("writing '{}'", path.display()))?;
    fs::set_permissions(file.path(), fs::Permissions::from_mode(0o755))
        .with_context(|| format!("setting the mode of '{}'", path.display()))?;
    file.persist(path)
        .with_context(|| format!("installing '{}'", path.display()))?;
    Ok(())
}
