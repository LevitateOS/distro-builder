//! The live ISO's install screen, from login to the split pane.
//!
//! The variant declares it (`[install_ux]` in products.toml). On each console it
//! lists, getty starts [`LAUNCHER`] in place of login (the live overlay writes those
//! getty lines, taking the console list and the launcher path from here). The
//! launcher runs [`SPLIT`] with the variant's shell on the left and the docs on the
//! right, and becomes a root login shell when the screen ends. A serial console runs
//! the screen only when the boot-injection payload sets [`SERIAL_KEY`]`=1`; otherwise
//! it gets the shell straight away.
//!
//! Paths are relative to the rootfs.

use anyhow::{bail, Context, Result};
use distro_contract::{InstallDocs, InstallUxContract};
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::pipeline::live_tools::{copy_executable, write_executable, write_text};

/// What getty starts on an install-screen console; `--probe` prints
/// `PROBE_PREFIX<split pane path>`.
pub const LAUNCHER: &str = "usr/local/bin/install-ux";
/// The split pane (`install-split-pane`'s `install-split`); a runtime action in the
/// variant installs it here.
pub const SPLIT: &str = "usr/local/bin/install-split";
/// The docs command, in the right pane and on the shell's PATH.
pub const DOCS: &str = "usr/local/bin/install-docs";
const DOCS_BUNDLE: &str = "usr/share/install-ux/install-docs.js";
const DOCS_TEXT: &str = "usr/share/install-ux/install-guide.txt";
const BUN: &str = "usr/local/bin/bun";
/// The boot-injection key that turns the install screen on for the serial console.
pub const SERIAL_KEY: &str = "INSTALL_UX_SERIAL";
pub const PROBE_PREFIX: &str = "install-entrypoint-helper=";
/// Set to 1, the launcher runs the split pane's `--smoke` instead.
pub const SMOKE_ENV: &str = "INSTALL_UX_SMOKE";
/// Printed by `install-split --smoke`.
pub const SPLIT_SMOKE_OK: &str = "split-smoke:ok";

/// The live `/etc/issue` login hint; the install tests treat it as the live
/// serial console being ready.
pub const LIVE_LOGIN_HINT: &str = "Login as 'root' (no password)";

/// Writes the install screen into `rootfs`: the docs, their command and the
/// launcher. The split pane must already be there.
pub(crate) fn write_payload(
    repo_root: &Path,
    rootfs: &Path,
    os_name: &str,
    install_ux: &InstallUxContract,
) -> Result<()> {
    if !rootfs.join(SPLIT).is_file() {
        bail!(
            "the install screen needs /{SPLIT}: declare a rootfs_workspace_binary action for \
             install-split-pane's install-split with that destination"
        );
    }

    let docs_argv = match &install_ux.docs {
        InstallDocs::TuiApp { app, page } => {
            write_bun_docs(repo_root, rootfs, app)
                .with_context(|| format!("bundling the docs app '{app}'"))?;
            let mut argv = vec![
                format!("/{DOCS}"),
                "--title".to_string(),
                format!("{os_name} install guide"),
            ];
            if let Some(page) = page {
                argv.extend(["--slug".to_string(), page.clone()]);
            }
            argv
        }
        InstallDocs::Text(guide) => {
            let text = fs::read_to_string(guide).with_context(|| {
                format!(
                    "reading the variant's install guide '{}' (install_ux.docs.text)",
                    guide.display()
                )
            })?;
            write_text(&rootfs.join(DOCS_TEXT), &text)?;
            let docs = crate::guest::render(
                crate::guest::DOCS_TEXT,
                &[("GUIDE", &format!("/{DOCS_TEXT}"))],
            )?;
            write_executable(&rootfs.join(DOCS), &docs)?;
            vec![format!("/{DOCS}")]
        }
    };

    let launcher = crate::guest::render(
        crate::guest::INSTALL_UX,
        &[
            ("SHELL", &install_ux.shell),
            ("SPLIT", &format!("/{SPLIT}")),
            ("DOCS_ARGV", &shell_words(&docs_argv)),
            ("SERIAL_KEY", SERIAL_KEY),
            ("BOOT_INJECTION_ENV", recinit::live::BOOT_INJECTION_ENV),
            ("PROBE", PROBE_PREFIX),
            ("SMOKE", SMOKE_ENV),
        ],
    )?;
    write_executable(&rootfs.join(LAUNCHER), &launcher)
}

/// Bundles the TUI app with bun into [`DOCS_BUNDLE`] and installs bun and [`DOCS`].
fn write_bun_docs(repo_root: &Path, rootfs: &Path, app: &str) -> Result<()> {
    let docs_app = distro_contract::tui_apps::find(repo_root, app)?;
    let bun = crate::process::which("bun").context(
        "bun is required to bundle the install docs: install it and put it on PATH (https://bun.sh)",
    )?;

    // The app is a member of the root JavaScript workspace; install it the one way xtask does.
    run(
        Command::new(&bun)
            .args(["install", "--frozen-lockfile"])
            .current_dir(repo_root),
        "bun install --frozen-lockfile at the repo root",
    )?;

    let bundle = rootfs.join(DOCS_BUNDLE);
    if let Some(parent) = bundle.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    run(
        Command::new(&bun)
            .arg("build")
            .arg(&docs_app.entry)
            .args(["--target=bun", "--minify", "--outfile"])
            .arg(&bundle)
            .current_dir(&docs_app.dir),
        &format!("bun build of '{}'", docs_app.dir.display()),
    )?;
    if !bundle.is_file() {
        bail!("bun build wrote no bundle at '{}'", bundle.display());
    }

    copy_executable(Path::new(&bun), &rootfs.join(BUN))?;
    let docs = crate::guest::render(
        crate::guest::DOCS_BUN,
        &[
            ("DOCS_BUNDLE", &format!("/{DOCS_BUNDLE}")),
            ("BUN", &format!("/{BUN}")),
        ],
    )?;
    write_executable(&rootfs.join(DOCS), &docs)
}

fn run(command: &mut Command, what: &str) -> Result<()> {
    let output = command
        .output()
        .with_context(|| format!("running {what}"))?;
    if !output.status.success() {
        bail!(
            "{what} failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout).trim(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// `argv` as one POSIX shell command line, each word single-quoted where needed.
fn shell_words(argv: &[String]) -> String {
    argv.iter()
        .map(|word| {
            if !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "/-_.=".contains(c))
            {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
