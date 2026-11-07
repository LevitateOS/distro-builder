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
//! Everything goes into the live overlay: the rootfs is what `recstrap` installs, and an
//! installed system has no use for the install screen. Paths are relative to a layer's root.

use anyhow::{bail, Context, Result};
use distro_contract::{InstallDocs, InstallUxContract, RuntimeActionContract};
use std::fs;
use std::path::Path;

use crate::artifact::filesystem::write_file;

/// What getty starts on an install-screen console; `--probe` prints
/// `PROBE_PREFIX<split pane path>`.
pub const LAUNCHER: &str = "usr/local/bin/install-ux";
/// The split pane (`install-split-pane`'s `install-split`); the variant's
/// `tool_payload_workspace_binary` action puts it here.
pub const SPLIT: &str = "usr/bin/install-split";
/// The docs command when the docs are a text guide (a TUI docs app is its own command,
/// under [`crate::tui_binary::BIN_DIR`]).
pub const DOCS: &str = "usr/local/bin/install-docs";
const DOCS_TEXT: &str = "usr/share/install-ux/install-guide.txt";
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

/// Writes the install screen into `overlay`: the TUI apps (the docs app among them when
/// the docs are one), the docs command and the launcher. The split pane must already be
/// there; `rootfs` is the root the overlay goes on.
pub(crate) fn write_payload(
    repo_root: &Path,
    rootfs: &Path,
    overlay: &Path,
    os_name: &str,
    install_ux: &InstallUxContract,
    runtime_actions: &[RuntimeActionContract],
) -> Result<()> {
    if !overlay.join(SPLIT).is_file() {
        bail!(
            "the install screen needs /{SPLIT}: declare a tool_payload_workspace_binary action for \
             install-split-pane's install-split"
        );
    }

    let mut app_names: Vec<&str> = install_ux.apps.iter().map(String::as_str).collect();
    if let InstallDocs::TuiApp { app, .. } = &install_ux.docs {
        if !app_names.contains(&app.as_str()) {
            app_names.insert(0, app);
        }
    }
    let apps = app_names
        .iter()
        .map(|name| Ok(distro_contract::tui_apps::find(repo_root, name)?))
        .collect::<Result<Vec<_>>>()?;
    for app in &apps {
        for tool in &app.tools {
            if !runtime_actions.iter().any(|action| ships(action, tool)) {
                bail!(
                    "the install screen's app '{}' runs {tool}, but the variant's live tools \
                     don't ship it: add a tool_payload_workspace_binary or \
                     rootfs_workspace_binary action for the '{tool}' package",
                    app.name
                );
            }
        }
    }
    crate::tui_binary::install(repo_root, rootfs, overlay, &apps)
        .context("building the install screen's TUI apps")?;

    let docs_argv = match &install_ux.docs {
        InstallDocs::TuiApp { app, page } => {
            let docs_app = apps
                .iter()
                .find(|a| &a.name == app)
                .expect("the docs app is among the apps");
            let mut argv = vec![
                format!("/{}/{app}", crate::tui_binary::BIN_DIR),
                "--title".to_string(),
                format!("{os_name} install guide"),
            ];
            if let Some(page) = page {
                // The flag that opens a page is the one the app's manifest declares.
                let pages = docs_app.inspect.pages.as_ref().with_context(|| {
                    format!("install_ux opens page '{page}', but {app} declares no inspect.pages")
                })?;
                argv.extend([pages.arg.clone(), page.clone()]);
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
            write_file(&overlay.join(DOCS_TEXT), text, 0o644)?;
            let docs = crate::guest::render(
                crate::guest::DOCS_TEXT,
                &[("GUIDE", &format!("/{DOCS_TEXT}"))],
            )?;
            write_file(&overlay.join(DOCS), docs, 0o755)?;
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
    write_file(&overlay.join(LAUNCHER), launcher, 0o755)
}

/// Whether `action` puts a binary of the Cargo `package` on the live ISO.
fn ships(action: &RuntimeActionContract, package: &str) -> bool {
    match action {
        RuntimeActionContract::ToolPayloadWorkspaceBinary { package: p, .. }
        | RuntimeActionContract::RootfsWorkspaceBinary { package: p, .. } => p == package,
    }
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
