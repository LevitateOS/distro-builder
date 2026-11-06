//! The live overlay: the live session's configuration, layered over the read-only
//! rootfs with overlayfs.
//!
//! One writer per file. Every live overlay has:
//! - `/etc/issue` (the caller's banner) and `/etc/shadow` (root with an empty password)
//! - the payload role file
//! - `etc/profile.d/20-live-shell.sh`: shell defaults (pager, editor, colours, prompt, history)
//! - `etc/profile.d/99-serial-test.sh`: recqemu's serial test protocol, last so its
//!   prompt and echo settings win on the serial console
//! - `live-help`
//! - getty autologin as root on the consoles, running the install screen's launcher
//!   on the install-UX consoles
//!
//! Then per init system. OpenRC: `/etc/inittab`, a tmpfs `/var/log` in `/etc/fstab`,
//! efivarfs, and suspend disabled (ACPI handler, sysctl, elogind). systemd: getty
//! drop-ins, the network-setup and shutdown-cleanup units, `/run/sshd`, and an empty
//! `/etc/machine-id` so each boot gets its own.
//!
//! The live-boot rootfs carries its own OpenRC inittab, serial autologin, issue and
//! shadow (`pipeline::overlay::ensure_openrc_shell`): `recstrap` installs that rootfs,
//! so those are what an installed system starts from. On the live ISO this overlay
//! replaces them.

use anyhow::{bail, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Which consoles an OpenRC inittab runs getty on.
#[derive(Debug, Clone, Copy)]
pub enum InittabVariant {
    /// Six VTs and the serial console.
    DesktopWithSerial,
    /// The serial console only.
    SerialOnly,
}

#[derive(Debug, Clone, Copy)]
pub enum InitSystem {
    OpenRc(InittabVariant),
    Systemd,
}

#[derive(Debug)]
pub struct LiveOverlayConfig<'a> {
    /// OS display name (e.g., "AcornOS").
    pub os_name: &'a str,
    /// The text of `/etc/issue`.
    pub issue: &'a str,
    pub init: InitSystem,
    /// Consoles whose getty starts the install screen instead of a shell.
    pub install_ux_consoles: &'a [String],
}

/// Writes the live overlay at `output_dir/live-overlay`, replacing any earlier one.
pub fn create_live_overlay(output_dir: &Path, config: &LiveOverlayConfig) -> Result<PathBuf> {
    let live_overlay = output_dir.join("live-overlay");
    if live_overlay.exists() {
        fs::remove_dir_all(&live_overlay)?;
    }
    fs::create_dir_all(live_overlay.join("etc/profile.d"))?;
    fs::create_dir_all(live_overlay.join("usr/local/bin"))?;

    fs::write(live_overlay.join("etc/issue"), config.issue)?;
    // lastchg is a recent day: 0 would demand a new password at the first login.
    write_with_mode(
        &live_overlay.join("etc/shadow"),
        "root::20000:0:99999:7:::\n\
         bin:!:0:0:99999:7:::\n\
         daemon:!:0:0:99999:7:::\n\
         nobody:!:0:0:99999:7:::\n",
        0o600,
    )?;
    fs::write(
        live_overlay.join(recinit::live::PAYLOAD_ROLE_FILE),
        format!("{}\n", recinit::live::ROLE_OVERLAY),
    )?;
    write_with_mode(
        &live_overlay.join("etc/profile.d/20-live-shell.sh"),
        crate::guest::SHELL_UX_PROFILE,
        0o755,
    )?;
    fs::write(
        live_overlay.join("etc/profile.d/99-serial-test.sh"),
        recqemu::serial::protocol::guest_profile(),
    )?;
    write_with_mode(
        &live_overlay.join("usr/local/bin/live-help"),
        &crate::guest::render(crate::guest::LIVE_HELP, &[("OS_NAME", config.os_name)])?,
        0o755,
    )?;

    match config.init {
        InitSystem::OpenRc(inittab) => write_openrc(&live_overlay, config, inittab)?,
        InitSystem::Systemd => write_systemd(&live_overlay, config)?,
    }
    Ok(live_overlay)
}

fn write_openrc(
    live_overlay: &Path,
    config: &LiveOverlayConfig,
    inittab: InittabVariant,
) -> Result<()> {
    let launcher = format!("/{}", crate::install_ux::LAUNCHER);
    let install_ux = |tty: &str| config.install_ux_consoles.iter().any(|c| c == tty);
    let vts: &[&str] = match inittab {
        InittabVariant::DesktopWithSerial => &["tty1", "tty2", "tty3", "tty4", "tty5", "tty6"],
        InittabVariant::SerialOnly => &[],
    };
    if let Some(console) = config
        .install_ux_consoles
        .iter()
        .find(|c| *c != "ttyS0" && !vts.contains(&c.as_str()))
    {
        bail!("install_ux console '{console}' has no getty in this inittab");
    }
    let vt_gettys: String = vts
        .iter()
        .map(|tty| {
            if install_ux(tty) {
                format!("{tty}::respawn:/sbin/getty -n -l {launcher} 38400 {tty} linux\n")
            } else {
                format!("{tty}::respawn:/sbin/getty 38400 {tty}\n")
            }
        })
        .collect();
    // Without the install screen, the rootfs's serial autologin gives a root shell.
    let serial_login = if install_ux("ttyS0") {
        launcher.as_str()
    } else {
        "/usr/local/bin/serial-autologin"
    };
    fs::write(
        live_overlay.join("etc/inittab"),
        format!(
            "# /etc/inittab - {os} Live\n\
             \n\
             ::sysinit:/sbin/openrc sysinit\n\
             ::sysinit:/sbin/openrc boot\n\
             \n\
             {vt_gettys}\
             ttyS0::respawn:/sbin/getty -L -n -l {serial_login} 115200 ttyS0 vt100\n\
             \n\
             ::wait:/sbin/openrc default\n\
             ::ctrlaltdel:/sbin/reboot\n\
             ::shutdown:/sbin/openrc shutdown\n",
            os = config.os_name
        ),
    )?;

    fs::create_dir_all(live_overlay.join("etc/runlevels/default"))?;
    fs::create_dir_all(live_overlay.join("etc/conf.d"))?;
    // Logs in RAM, so they can't fill the overlay; localmount mounts it at boot.
    fs::write(
        live_overlay.join("etc/fstab"),
        "tmpfs   /var/log    tmpfs   nosuid,nodev,noexec,size=64M,mode=0755   0 0\n",
    )?;
    fs::create_dir_all(live_overlay.join("etc/local.d"))?;
    write_with_mode(
        &live_overlay.join("etc/local.d/01-efivarfs.start"),
        crate::guest::EFIVARFS_START,
        0o755,
    )?;

    // No suspend during an install: ACPI events, sysctl and elogind.
    fs::create_dir_all(live_overlay.join("etc/acpi"))?;
    write_with_mode(
        &live_overlay.join("etc/acpi/handler.sh"),
        &crate::guest::render(crate::guest::ACPI_HANDLER, &[("OS_NAME", config.os_name)])?,
        0o755,
    )?;
    fs::create_dir_all(live_overlay.join("etc/sysctl.d"))?;
    fs::write(
        live_overlay.join("etc/sysctl.d/50-live-no-suspend.conf"),
        format!(
            "# {} Live: Disable suspend\n\
             # Prevent accidental suspend during installation\n\
             \n\
             # Disable suspend-to-RAM\n\
             kernel.sysrq = 1\n",
            config.os_name
        ),
    )?;
    fs::create_dir_all(live_overlay.join("etc/elogind/logind.conf.d"))?;
    fs::write(
        live_overlay.join("etc/elogind/logind.conf.d/00-live-no-suspend.conf"),
        format!(
            "# {} Live: Disable suspend triggers\n\
             [Login]\n\
             HandlePowerKey=ignore\n\
             HandleSuspendKey=ignore\n\
             HandleHibernateKey=ignore\n\
             HandleLidSwitch=ignore\n\
             HandleLidSwitchExternalPower=ignore\n\
             HandleLidSwitchDocked=ignore\n\
             IdleAction=ignore\n",
            config.os_name
        ),
    )?;
    Ok(())
}

fn write_systemd(live_overlay: &Path, config: &LiveOverlayConfig) -> Result<()> {
    let system = live_overlay.join("etc/systemd/system");
    for dir in [
        "getty@.service.d",
        "serial-getty@.service.d",
        "getty.target.wants",
        "basic.target.wants",
        "multi-user.target.wants",
    ] {
        fs::create_dir_all(system.join(dir))?;
    }
    fs::create_dir_all(live_overlay.join("etc/tmpfiles.d"))?;
    fs::create_dir_all(live_overlay.join("usr/local/sbin"))?;

    // Autologin as root: one template drop-in for the VTs, one for serial consoles.
    // An install-screen console gets an instance drop-in of the same name, which
    // replaces the template's and runs the launcher in place of login.
    if let Some(console) = config
        .install_ux_consoles
        .iter()
        .find(|c| !(c.starts_with("ttyS") || c.as_str() == "tty1"))
    {
        bail!("install_ux console '{console}' has no getty in the systemd live overlay");
    }
    let vt = |login: &str| {
        format!("[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noclear{login} %I $TERM\n")
    };
    let serial = |login: &str| {
        format!("[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root{login} --keep-baud 115200,57600,38400,9600 %I vt100\n")
    };
    fs::write(system.join("getty@.service.d/zz-autologin.conf"), vt(""))?;
    fs::write(
        system.join("serial-getty@.service.d/zz-autologin.conf"),
        serial(""),
    )?;
    let launcher = format!(" --login-program /{}", crate::install_ux::LAUNCHER);
    for console in config.install_ux_consoles {
        let (unit, drop_in) = if console.starts_with("ttyS") {
            (
                format!("serial-getty@{console}.service.d"),
                serial(&launcher),
            )
        } else {
            (format!("getty@{console}.service.d"), vt(&launcher))
        };
        fs::create_dir_all(system.join(&unit))?;
        fs::write(system.join(unit).join("zz-autologin.conf"), drop_in)?;
    }
    symlink(
        "/usr/lib/systemd/system/getty@.service",
        system.join("getty.target.wants/getty@tty1.service"),
    )?;
    for target in ["basic.target.wants", "getty.target.wants"] {
        symlink(
            "/usr/lib/systemd/system/serial-getty@.service",
            system.join(target).join("serial-getty@ttyS0.service"),
        )?;
    }

    // NIC bring-up for QEMU user networking (SSH via hostfwd): the first
    // non-loopback interface, whatever it is called.
    write_with_mode(
        &live_overlay.join("usr/local/sbin/live-net-setup"),
        crate::guest::LIVE_NET_SETUP,
        0o755,
    )?;
    fs::write(
        system.join("live-net-setup.service"),
        format!(
            "[Unit]\n\
             Description={} live network setup (slirp SSH)\n\
             DefaultDependencies=no\n\
             After=basic.target local-fs.target\n\
             Before=network.target network-online.target sshd.service\n\
             Wants=network.target\n\
             \n\
             [Service]\n\
             Type=oneshot\n\
             RemainAfterExit=yes\n\
             ExecStart=/usr/local/sbin/live-net-setup\n\
             \n\
             [Install]\n\
             WantedBy=multi-user.target\n",
            config.os_name
        ),
    )?;
    symlink(
        "/etc/systemd/system/live-net-setup.service",
        system.join("multi-user.target.wants/live-net-setup.service"),
    )?;

    write_with_mode(
        &live_overlay.join("usr/local/sbin/live-shutdown-cleanup"),
        crate::guest::LIVE_SHUTDOWN_CLEANUP,
        0o755,
    )?;
    fs::write(
        system.join("live-shutdown-cleanup.service"),
        "[Unit]\n\
         Description=Live ISO shutdown cleanup\n\
         DefaultDependencies=no\n\
         After=multi-user.target\n\
         Before=run-live-media.mount umount.target shutdown.target\n\
         Conflicts=shutdown.target umount.target\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         RemainAfterExit=yes\n\
         ExecStart=/bin/true\n\
         ExecStop=/usr/local/sbin/live-shutdown-cleanup\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
    )?;
    symlink(
        "/etc/systemd/system/live-shutdown-cleanup.service",
        system.join("multi-user.target.wants/live-shutdown-cleanup.service"),
    )?;

    // sshd needs its runtime directory on the tmpfs /run.
    fs::write(
        live_overlay.join("etc/tmpfiles.d/sshd-local.conf"),
        "d /run/sshd 0755 root root -\n",
    )?;
    // Empty: systemd makes up a machine ID at each boot.
    fs::write(live_overlay.join("etc/machine-id"), "")?;
    Ok(())
}

fn write_with_mode(path: &Path, content: &str, mode: u32) -> Result<()> {
    fs::write(path, content)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}
