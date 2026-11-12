//! The live session a live product's overlay sets up, made by Recipe's
//! `os-recipes/live-overlay.rhai`. Nothing here or there writes the rootfs.
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use distro_contract::profile::{Flavour, InitSystem};
use distro_contract::{ProductId, Variant};
use serde::Serialize;

use crate::guest::{self, Value};

/// The live session of a live product: the `config` input of
/// `os-recipes/live-overlay.rhai`, which reads these fields by name.
#[derive(Debug, Serialize)]
pub(crate) struct LiveSession {
    /// The `/etc/issue` banner is `<os_name> <label>`, with `hint` under it.
    os_name: String,
    label: &'static str,
    hint: &'static str,
    init: InitSystem,
    /// The consoles that run the install screen, `launcher`; root logs in
    /// without asking on every other one.
    consoles: Vec<String>,
    launcher: Option<String>,
    /// Units (systemd) or services (OpenRC) the session starts besides the
    /// installed system's.
    services: Vec<String>,
    /// On the desktop system: its display manager, which the live installer
    /// does not start (the install screen has tty1, as on every live ISO; the
    /// desktop starts once the system is installed).
    desktop: Option<LiveInstallerDesktop>,
    /// Where recinit's live init keeps the boot medium mounted.
    live_media_dir: &'static str,
    /// The overlay's payload role file (recinit's payload-role protocol).
    role_file: &'static str,
    role: &'static str,
}

/// The desktop system under the live installer: the display manager it
/// keeps off. On OpenRC the overlay boots a `live` runlevel made of the
/// installed system's `default` runlevel (`default_runlevel`, read from the
/// rootfs when the overlay is made) without the display manager; on systemd
/// it boots `multi-user.target`.
#[derive(Debug, Serialize)]
struct LiveInstallerDesktop {
    display_manager: String,
    default_runlevel: Vec<String>,
}

impl LiveSession {
    /// `product`'s session, from the variant's live and install-screen config.
    pub(crate) fn new(variant: &Variant, product: ProductId) -> Self {
        let contract = &variant.contract;
        let build = crate::product::build(product);
        let install_screen = build.install_screen;
        Self {
            os_name: contract.identity.os_name.clone(),
            label: product.title(),
            hint: crate::install_ux::LIVE_LOGIN_HINT,
            init: contract.profile.init,
            consoles: if install_screen {
                crate::install_ux::consoles()
            } else {
                Vec::new()
            },
            launcher: install_screen.then(|| format!("/{}", crate::install_ux::LAUNCHER)),
            services: contract.live_service_units(),
            desktop: (product.flavour() == Flavour::Desktop).then(|| LiveInstallerDesktop {
                display_manager: contract.desktop.display_manager.clone(),
                default_runlevel: Vec::new(),
            }),
            live_media_dir: recinit::live::LIVE_MEDIA_DIR,
            role_file: recinit::live::PAYLOAD_ROLE_FILE,
            role: recinit::live::ROLE_OVERLAY,
        }
    }
}

/// Writes `session`'s overlay into `overlay` (an empty directory beside
/// `rootfs`, the system it goes on).
pub(crate) fn create_live_overlay(
    variant: &Variant,
    session: &LiveSession,
    rootfs: &Path,
    overlay: &Path,
) -> Result<()> {
    let repo = &variant.repo_root;
    let parent = overlay.parent().unwrap_or(overlay);
    let inputs = tempfile::tempdir_in(parent)
        .with_context(|| format!("creating the live overlay's inputs under {}", parent.display()))?;
    let write = |name: &str, text: String| -> Result<PathBuf> {
        let path = inputs.path().join(name);
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    };
    let mut config = serde_json::to_value(session)?;
    if session.desktop.is_some() && session.init == InitSystem::Openrc {
        config["desktop"]["default_runlevel"] = serde_json::to_value(default_runlevel(rootfs)?)?;
    }
    let config = write("config", serde_json::to_string(&config)?)?;
    let values = [
        ("OS_NAME", Value::Text(&session.os_name)),
        ("LIVE_MEDIA_DIR", Value::Word(session.live_media_dir)),
    ];
    let mut invocation = crate::recipe::Invocation::new(
        repo,
        "install",
        repo.join("distro/builder/os-recipes/live-overlay.rhai"),
        crate::recipe::recipe_work_dir(repo, &variant.id)?.join("live-overlay"),
    )
    .input("config", &config)
    .input("shadow", &rootfs.join("etc/shadow"));
    for (input, file) in [
        ("shell_ux", &guest::LIVE_SHELL),
        ("live_help", &guest::LIVE_HELP),
        ("shutdown_cleanup", &guest::LIVE_SHUTDOWN_CLEANUP),
    ] {
        let path = write(input, guest::render(file, &values)?)?;
        invocation = invocation.input(input, &path);
    }
    // The serial test protocol's guest profile. recqemu owns the protocol until
    // its marker moves to recinit::live and the harness switches the profile on
    // through boot injection.
    let serial_profile = write("serial_profile", recqemu::serial::protocol::guest_profile())?;
    let result = invocation
        .input("serial_profile", &serial_profile)
        .run()
        .context("preparing the live overlay with Recipe")?;
    levitate_recipe::target::merge_tree(&result.directory("overlay")?, overlay)
}

/// The services in the OpenRC `default` runlevel of the system at `rootfs`,
/// by name, in name order.
fn default_runlevel(rootfs: &Path) -> Result<Vec<String>> {
    let dir = rootfs.join("etc/runlevels/default");
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let name = entry?.file_name();
        names.push(name.to_str().context("non-UTF-8 runlevel entry")?.to_string());
    }
    names.sort();
    Ok(names)
}
