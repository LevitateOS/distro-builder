//! The live ISO's install screen, from login to the split pane.
//!
//! The screen is the same on every variant (`distro_contract::install_ux`): the
//! hub and the docs side by side in the split pane, and the other install apps
//! as commands. On each console it lists, getty starts the launcher in place of
//! login (the live overlay writes those getty lines). The launcher runs the split
//! pane with the hub on the left and the docs on the right, and becomes a root
//! login shell when the screen ends. A serial console runs the screen only when
//! the boot-injection payload sets `SERIAL_KEY=1`; otherwise it gets the shell
//! straight away.
//!
//! Everything goes into the live-tools overlay (the layer `pipeline/products.rs` adds
//! for a product with `install_screen`), never into the system rootfs below it, which
//! has no use for the install screen. Paths are relative to a layer's root.

use anyhow::{bail, Context, Result};
use distro_contract::install_ux::{APPS, BIN_DIR, CONSOLES, DOCS, DOCS_PAGE, DOCS_PANE, HUB, SHELL};
use distro_contract::profile::Appearance;
use distro_contract::{LookContract, Profile};
use std::path::Path;

use crate::artifact::filesystem::write_file;
use crate::tui_apps::{TuiApp, Workspace};

pub use distro_contract::install_ux::{
    LAUNCHER, PROBE_PREFIX, SERIAL_KEY, SMOKE_ENV, SMOKE_OK as SPLIT_SMOKE_OK, SPLIT,
};

/// The line under the live `/etc/issue` banner. Every live console logs root
/// in without a password (the install screen's too), so the hint says so;
/// the install tests treat it as the live serial console being ready.
pub const LIVE_LOGIN_HINT: &str = "root is logged in on every console (no password)";

/// The screen's apps, resolved from the workspace.
pub struct Screen<'a> {
    pub hub: &'a TuiApp,
    pub docs: &'a TuiApp,
    /// The other apps, commands for the shell beside the docs.
    pub extra: Vec<&'a TuiApp>,
}

impl<'a> Screen<'a> {
    /// The screen's apps in `workspace`. Each must exist, the hub and the docs must
    /// take the distro's name by a declared flag (the hub its shell too), and the
    /// docs must declare how a page is opened.
    pub fn resolve(workspace: &'a Workspace) -> Result<Self> {
        let screen = Self {
            hub: workspace.app(HUB)?,
            docs: workspace.app(DOCS)?,
            extra: APPS.iter().map(|name| workspace.app(name)).collect::<Result<_>>()?,
        };
        for (app, shell) in [(screen.hub, true), (screen.docs, false)] {
            let flags = &app.distro_args;
            if flags.os_name.is_none() || (shell && flags.shell.is_none()) {
                bail!(
                    "the install screen tells {} the distro's name{}: its package.json must \
                     declare levitate.distroArgs.osName{}",
                    app.name,
                    if shell { " and shell" } else { "" },
                    if shell { " and .shell" } else { "" },
                );
            }
        }
        screen.docs.page_arg()?;
        Ok(screen)
    }

    /// Every app the screen's TUI binary carries.
    pub fn apps(&self) -> Vec<&'a TuiApp> {
        [self.hub, self.docs].into_iter().chain(self.extra.iter().copied()).collect()
    }

    /// The pane commands on `os_name`: the hub's, and the docs' at their page.
    pub fn panes(&self, os_name: &str) -> Result<[Vec<String>; 2]> {
        let command = |app: &TuiApp| {
            let mut argv = vec![format!("/{BIN_DIR}/{}", app.name)];
            argv.extend(app.distro_argv(os_name, SHELL));
            argv
        };
        let mut docs = command(self.docs);
        docs.extend([DOCS_PANE.to_string(), self.docs.page_arg()?.to_string(), DOCS_PAGE.to_string()]);
        Ok([command(self.hub), docs])
    }
}

/// Fails, before anything is built, if the screen's apps don't resolve.
pub(crate) fn check(repo_root: &Path) -> Result<()> {
    Screen::resolve(&Workspace::load(repo_root)?).map(drop)
}

/// The consoles whose getty starts the launcher.
pub(crate) fn consoles() -> Vec<String> {
    CONSOLES.iter().map(|console| console.to_string()).collect()
}

/// The split pane's colours: `--look ROLE=#RRGGBB` for each role it draws with, from
/// the palette of the mode the variant starts in. Roles, not names: the key bar is
/// `surface` (as the apps' own key bars) with `text_muted` labels, keycaps `text` on `rule`, a pane's
/// notice `warn`, and a dimmed pane fades toward `surface`.
fn split_look_args(appearance: Appearance, look: &LookContract) -> Vec<String> {
    let palette = match appearance {
        Appearance::Light => &look.light,
        Appearance::Dark => &look.dark,
    };
    [
        ("surface", &palette.surface),
        ("text", &palette.text),
        ("text_muted", &palette.text_muted),
        ("rule", &palette.rule),
        ("warn", &palette.warn),
    ]
    .into_iter()
    .flat_map(|(role, hex)| ["--look".to_string(), format!("{role}={hex}")])
    .collect()
}

/// Writes the install screen into `overlay`: the TUI apps and the launcher. The
/// split pane must already be there; `rootfs` is the root the overlay goes on.
pub(crate) fn write_payload(
    repo_root: &Path,
    rootfs: &Path,
    overlay: &Path,
    os_name: &str,
    profile: &Profile,
    look: &LookContract,
) -> Result<()> {
    if !overlay.join(SPLIT).is_file() {
        bail!(
            "the install screen needs /{SPLIT}: the builder's live tools \
             (pipeline/live_tools.rs) must carry install-split-pane's install-split"
        );
    }

    let workspace = Workspace::load(repo_root)?;
    let screen = Screen::resolve(&workspace)?;
    let apps = screen.apps();
    for app in &apps {
        for tool in &app.tools {
            let shipped = crate::pipeline::live_tools::LIVE_TOOLS
                .iter()
                .any(|live| live.binary.unwrap_or(live.package) == tool);
            if !shipped {
                bail!(
                    "the install screen's app '{}' runs {tool}, but the live tools don't ship \
                     a binary called that: add it to the builder's live tools \
                     (pipeline/live_tools.rs)",
                    app.name
                );
            }
        }
    }
    crate::tui_binary::install(
        repo_root,
        rootfs,
        overlay,
        crate::tui_binary::INSTALL_BINARY,
        profile.libc,
        &apps,
    )
        .context("building the install screen's TUI apps")?;

    let [hub, docs] = screen.panes(os_name)?;
    let look_args = split_look_args(profile.appearance, look);
    let serial: Vec<String> = CONSOLES
        .iter()
        .filter(|console| console.starts_with("ttyS"))
        .map(|console| format!("/dev/{console}"))
        .collect();
    use crate::guest::Value::{Argv, Word};
    let split = format!("/{SPLIT}");
    let launcher = crate::guest::render(
        &crate::guest::INSTALL_UX,
        &[
            ("SHELL", Word(SHELL)),
            ("LEFT", Argv(&hub)),
            ("SPLIT", Word(&split)),
            ("LOOK", Argv(&look_args)),
            ("DOCS_ARGV", Argv(&docs)),
            ("SERIAL_CONSOLES", Argv(&serial)),
            ("SERIAL_KEY", Word(SERIAL_KEY)),
            ("BOOT_INJECTION_ENV", Word(recinit::live::BOOT_INJECTION_ENV)),
            ("PROBE", Word(PROBE_PREFIX)),
            ("SMOKE", Word(SMOKE_ENV)),
        ],
    )?;
    write_file(&overlay.join(LAUNCHER), launcher, 0o755)
}
