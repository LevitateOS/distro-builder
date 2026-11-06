//! Shell scripts the builder writes into the guest.
//!
//! Each script is a real file under `guest/` (so it can be read, run and syntax-checked
//! as shell), with `@NAME@` placeholders for values only known at build time.
//! [`render`] fills them and refuses a template with a placeholder it was not given.

use anyhow::{bail, Result};

macro_rules! scripts {
    ($($name:ident = $path:literal;)*) => {
        $(pub(crate) const $name: &str = include_str!(concat!("../guest/", $path));)*
    };
}

scripts! {
    ACPI_HANDLER = "live/acpi-handler.sh";
    EFIVARFS_START = "live/efivarfs.start";
    LIVE_HELP = "live/live-help.sh";
    LIVE_NET_SETUP = "live/live-net-setup.sh";
    LIVE_SHUTDOWN_CLEANUP = "live/live-shutdown-cleanup.sh";
    SERIAL_AUTOLOGIN_OPENRC = "live/serial-autologin.openrc.sh";
    SERIAL_AUTOLOGIN_ROOTFS = "live/serial-autologin.rootfs.sh";
    SHELL_COLOR_PROFILE = "live/shell-color.sh";
    SHELL_UX_PROFILE = "live/shell-ux.sh";
    VOLATILE_LOG_START = "live/volatile-log.start";
    DOCS_BUN = "install-ux/docs-bun.sh";
    DOCS_TEXT = "install-ux/docs-text.sh";
    INSTALL_UX = "install-ux/install-ux.sh";
}

/// Fills every `@NAME@` in `template` from `values`. A placeholder with no value is an
/// error (a misspelt name never reaches the guest as literal text); values the
/// template does not use are ignored.
pub(crate) fn render(template: &str, values: &[(&str, &str)]) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('@') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_len = after
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
            .unwrap_or(after.len());
        if name_len > 0 && after[name_len..].starts_with('@') {
            let name = &after[..name_len];
            let Some((_, value)) = values.iter().find(|(key, _)| *key == name) else {
                bail!("guest script placeholder @{name}@ has no value");
            };
            out.push_str(value);
            rest = &after[name_len + 1..];
        } else {
            out.push('@');
            rest = after;
        }
    }
    out.push_str(rest);
    Ok(out)
}

/// Renders a script whose only placeholder is the serial readiness marker.
pub(crate) fn render_boot(template: &str) -> Result<String> {
    render(
        template,
        &[(
            "SHELL_READY_MARKER",
            recqemu::serial::protocol::SHELL_READY_MARKER,
        )],
    )
}
