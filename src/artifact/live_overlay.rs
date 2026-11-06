//! Shared live overlay builders.
//!
//! This module provides:
//! - OpenRC live overlay generation (AcornOS style)
//! - Systemd live overlay generation (LevitateOS style)

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::copy_dir_recursive;

/// Inittab variant controlling which consoles are enabled.
#[derive(Debug, Clone, Copy)]
pub enum InittabVariant {
    /// Desktop: 6 VTYs + serial console with autologin (AcornOS).
    DesktopWithSerial,
    /// Headless appliance: serial console only.
    SerialOnly,
}

/// Configuration for creating an OpenRC live overlay.
#[derive(Debug)]
pub struct LiveOverlayConfig<'a> {
    /// OS display name (e.g., "AcornOS").
    pub os_name: &'a str,
    /// Which inittab variant to generate.
    pub inittab: InittabVariant,
    /// Optional path to a variant-local live overlay seed directory to copy first.
    pub seed_overlay: Option<&'a Path>,
    /// The text of `/etc/issue`.
    pub issue: &'a str,
}

/// Configuration for creating a systemd live overlay.
#[derive(Debug)]
pub struct SystemdLiveOverlayConfig<'a> {
    /// OS display name (e.g., "LevitateOS").
    pub os_name: &'a str,
    /// The text of `/etc/issue`.
    pub issue: &'a str,
    /// Systemd unit names to mask by linking to `/dev/null`.
    pub masked_units: &'a [&'a str],
    /// Whether to write a serial-console scenario test marker profile script.
    pub write_serial_test_profile: bool,
    /// Optional machine-id content to write at `/etc/machine-id`.
    pub machine_id: Option<&'a str>,
    /// When true, install a strict UTF-8 locale profile for live shells.
    /// Live-rootfs producers must ensure UTF-8 locale payload exists in rootfs.
    pub enforce_utf8_locale_profile: bool,
}

/// Create an OpenRC live overlay at `output_dir/live-overlay`.
///
/// The overlay contains live-session-specific configuration that sits
/// on top of the read-only EROFS rootfs via overlayfs.
pub fn create_openrc_live_overlay(
    output_dir: &Path,
    config: &LiveOverlayConfig,
) -> Result<PathBuf> {
    println!("Creating live overlay...");

    let live_overlay = output_dir.join("live-overlay");

    // Clean previous
    if live_overlay.exists() {
        fs::remove_dir_all(&live_overlay)?;
    }

    // Step 1: Copy variant-local live overlay seed files (test instrumentation, etc.)
    if let Some(seed_overlay) = config.seed_overlay {
        if seed_overlay.exists() {
            println!("  Copying live overlay seed files...");
            copy_dir_recursive(seed_overlay, &live_overlay).with_context(|| {
                format!(
                    "Failed to copy {} -> {}",
                    seed_overlay.display(),
                    live_overlay.display()
                )
            })?;
        }
    }

    // Step 2: Code-generated overlay files
    fs::create_dir_all(live_overlay.join("etc")).with_context(|| "Failed to create etc")?;
    write_serial_test_profile(&live_overlay)?;

    // Serial autologin script
    fs::create_dir_all(live_overlay.join("usr/local/bin"))?;
    let autologin_script = crate::guest::render_boot(crate::guest::SERIAL_AUTOLOGIN_OPENRC)?;
    write_executable(
        &live_overlay.join("usr/local/bin/serial-autologin"),
        &autologin_script,
    )?;

    // /etc/issue
    fs::write(live_overlay.join("etc/issue"), config.issue)?;
    fs::write(
        live_overlay.join(recinit::live::PAYLOAD_ROLE_FILE),
        format!("{}\n", recinit::live::ROLE_OVERLAY),
    )?;

    // Empty root password for live session
    let shadow_content = "root::0:0:99999:7:::\n\
                          bin:!:0:0:99999:7:::\n\
                          daemon:!:0:0:99999:7:::\n\
                          nobody:!:0:0:99999:7:::\n";
    fs::write(live_overlay.join("etc/shadow"), shadow_content)?;
    let mut perms = fs::metadata(live_overlay.join("etc/shadow"))?.permissions();
    perms.set_mode(0o640);
    fs::set_permissions(live_overlay.join("etc/shadow"), perms)?;

    // Inittab
    let inittab_content = match config.inittab {
        InittabVariant::DesktopWithSerial => format!(
            r#"# /etc/inittab - {} Live

::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot

# Virtual terminals
tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6

# Serial console with autologin for test harness
# Uses wrapper script that spawns ash as login shell (sources /etc/profile.d/*)
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100

# Continue remaining services after serial shell is available
::wait:/sbin/openrc default

# Ctrl+Alt+Del
::ctrlaltdel:/sbin/reboot

# Shutdown
::shutdown:/sbin/openrc shutdown
"#,
            config.os_name
        ),
        InittabVariant::SerialOnly => format!(
            r#"# /etc/inittab - {} Live (headless appliance)

::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot

# Serial console PRIMARY with autologin (ttyS0) - appliance has no display
# Uses wrapper script that spawns ash as login shell (sources /etc/profile.d/*)
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100

# Continue remaining services after serial shell is available
::wait:/sbin/openrc default

# Ctrl+Alt+Del
::ctrlaltdel:/sbin/reboot

# Shutdown
::shutdown:/sbin/openrc shutdown
"#,
            config.os_name
        ),
    };
    fs::write(live_overlay.join("etc/inittab"), inittab_content)?;

    // Runlevels and conf.d directories
    fs::create_dir_all(live_overlay.join("etc/runlevels/default"))?;
    fs::create_dir_all(live_overlay.join("etc/conf.d"))?;

    // Volatile log storage
    let fstab_content = format!(
        "# {} Live fstab\n\
         # Volatile log storage - prevents logs from filling overlay tmpfs\n\
         tmpfs   /var/log    tmpfs   nosuid,nodev,noexec,size=64M,mode=0755   0 0\n",
        config.os_name
    );
    fs::write(live_overlay.join("etc/fstab"), fstab_content)?;

    // local.d scripts
    fs::create_dir_all(live_overlay.join("etc/local.d"))?;

    let volatile_log_script = crate::guest::VOLATILE_LOG_START;
    write_executable(
        &live_overlay.join("etc/local.d/00-volatile-log.start"),
        volatile_log_script,
    )?;

    let efivars_script = crate::guest::EFIVARFS_START;
    write_executable(
        &live_overlay.join("etc/local.d/01-efivarfs.start"),
        efivars_script,
    )?;

    // Do-not-suspend configuration
    // Method 1: ACPI handler
    fs::create_dir_all(live_overlay.join("etc/acpi"))?;
    let acpi_handler =
        crate::guest::render(crate::guest::ACPI_HANDLER, &[("OS_NAME", config.os_name)])?;
    write_executable(&live_overlay.join("etc/acpi/handler.sh"), &acpi_handler)?;

    // Method 2: sysctl
    fs::create_dir_all(live_overlay.join("etc/sysctl.d"))?;
    let sysctl_content = format!(
        "# {} Live: Disable suspend\n\
         # Prevent accidental suspend during installation\n\
         \n\
         # Disable suspend-to-RAM\n\
         kernel.sysrq = 1\n\
         \n\
         # Note: Full suspend disable requires either:\n\
         # - elogind HandleLidSwitch=ignore (if using elogind)\n\
         # - acpid handler (provided above)\n\
         # - Or simply not having any suspend triggers\n",
        config.os_name
    );
    fs::write(
        live_overlay.join("etc/sysctl.d/50-live-no-suspend.conf"),
        sysctl_content,
    )?;

    // Method 3: elogind config
    fs::create_dir_all(live_overlay.join("etc/elogind/logind.conf.d"))?;
    let logind_conf = format!(
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
    );
    fs::write(
        live_overlay.join("etc/elogind/logind.conf.d/00-live-no-suspend.conf"),
        logind_conf,
    )?;

    println!("  Live overlay created at {}", live_overlay.display());
    Ok(live_overlay)
}

/// Create a systemd live overlay at `output_dir/live-overlay`.
///
/// The overlay includes:
/// - tty1 autologin drop-in
/// - serial-getty template autologin drop-in
/// - empty root password for live session (`/etc/shadow`)
/// - `/etc/issue` banner
pub fn create_systemd_live_overlay(
    output_dir: &Path,
    config: &SystemdLiveOverlayConfig,
) -> Result<PathBuf> {
    println!("Creating systemd live overlay...");

    let live_overlay = output_dir.join("live-overlay");
    if live_overlay.exists() {
        fs::remove_dir_all(&live_overlay)?;
    }

    fs::create_dir_all(live_overlay.join("etc/systemd/system/getty@tty1.service.d"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/system/getty@.service.d"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/system/serial-getty@.service.d"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/system/getty.target.wants"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/system/basic.target.wants"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/system/multi-user.target.wants"))?;
    fs::create_dir_all(live_overlay.join("etc/systemd/network"))?;
    fs::create_dir_all(live_overlay.join("etc/profile.d"))?;
    fs::create_dir_all(live_overlay.join("etc/tmpfiles.d"))?;
    fs::create_dir_all(live_overlay.join("usr/local/bin"))?;
    fs::create_dir_all(live_overlay.join("usr/local/sbin"))?;

    let live_help_script = crate::guest::LIVE_HELP;
    write_executable(
        &live_overlay.join("usr/local/bin/live-help"),
        live_help_script,
    )?;

    let shell_ux_profile = crate::guest::SHELL_UX_PROFILE;
    write_executable(
        &live_overlay.join("etc/profile.d/20-live-shell-ux.sh"),
        shell_ux_profile,
    )?;

    let tty1_autologin =
        "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noclear %I $TERM\n";
    fs::write(
        live_overlay.join("etc/systemd/system/getty@tty1.service.d/autologin.conf"),
        tty1_autologin,
    )?;

    let serial_autologin_script =
        crate::guest::render_boot(crate::guest::SERIAL_AUTOLOGIN_SYSTEMD)?;
    write_executable(
        &live_overlay.join("usr/local/bin/serial-autologin"),
        &serial_autologin_script,
    )?;

    let serial_autologin = "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --keep-baud 115200,57600,38400,9600 %I vt100\n";
    fs::write(
        live_overlay.join("etc/systemd/system/serial-getty@.service.d/zz-autologin.conf"),
        serial_autologin,
    )?;
    fs::write(
        live_overlay.join("etc/systemd/system/getty@.service.d/zz-autologin.conf"),
        serial_autologin,
    )?;
    symlink(
        "/usr/lib/systemd/system/getty@.service",
        live_overlay.join("etc/systemd/system/getty.target.wants/getty@tty1.service"),
    )?;
    symlink(
        "/usr/lib/systemd/system/serial-getty@.service",
        live_overlay.join("etc/systemd/system/basic.target.wants/serial-getty@ttyS0.service"),
    )?;
    symlink(
        "/usr/lib/systemd/system/serial-getty@.service",
        live_overlay.join("etc/systemd/system/getty.target.wants/serial-getty@ttyS0.service"),
    )?;
    // Deterministic live NIC bring-up for slirp hostfwd SSH (QEMU usernet defaults).
    // Resolve the first non-loopback NIC dynamically to avoid brittle interface names
    // (e.g. ens3 vs ens4 depending on device ordering).
    let live_net_setup_script = crate::guest::LIVE_NET_SETUP;
    write_executable(
        &live_overlay.join("usr/local/sbin/live-net-setup"),
        live_net_setup_script,
    )?;

    let live_net_setup_unit = r#"[Unit]
Description=Levitate live network setup (slirp SSH)
DefaultDependencies=no
After=basic.target local-fs.target
Before=network.target network-online.target sshd.service
Wants=network.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/usr/local/sbin/live-net-setup

[Install]
WantedBy=multi-user.target
"#;
    fs::write(
        live_overlay.join("etc/systemd/system/live-net-setup.service"),
        live_net_setup_unit,
    )?;
    symlink(
        "/etc/systemd/system/live-net-setup.service",
        live_overlay.join("etc/systemd/system/multi-user.target.wants/live-net-setup.service"),
    )?;
    for unit in config.masked_units {
        symlink(
            "/dev/null",
            live_overlay.join(format!("etc/systemd/system/{unit}")),
        )?;
    }

    let shutdown_cleanup_script = crate::guest::LIVE_SHUTDOWN_CLEANUP;
    write_executable(
        &live_overlay.join("usr/local/sbin/live-shutdown-cleanup"),
        shutdown_cleanup_script,
    )?;

    let shutdown_cleanup_unit = r#"[Unit]
Description=Live ISO shutdown cleanup
DefaultDependencies=no
After=multi-user.target
Before=run-live-media.mount umount.target shutdown.target
Conflicts=shutdown.target umount.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/true
ExecStop=/usr/local/sbin/live-shutdown-cleanup

[Install]
WantedBy=multi-user.target
"#;
    fs::write(
        live_overlay.join("etc/systemd/system/live-shutdown-cleanup.service"),
        shutdown_cleanup_unit,
    )?;
    symlink(
        "/etc/systemd/system/live-shutdown-cleanup.service",
        live_overlay
            .join("etc/systemd/system/multi-user.target.wants/live-shutdown-cleanup.service"),
    )?;

    fs::write(live_overlay.join("etc/issue"), config.issue)?;

    if config.write_serial_test_profile {
        write_serial_test_profile(&live_overlay)?;
    }

    if config.enforce_utf8_locale_profile {
        // Live override: use portable C locale for minimal rootfs payloads.
        let locale_profile = r#"#!/bin/sh
# Live override: use portable C locale (always available).
export LANG=C
unset LC_ALL LC_CTYPE LC_NUMERIC LC_TIME LC_COLLATE LC_MESSAGES \
      LC_MONETARY LC_PAPER LC_NAME LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT \
      LC_IDENTIFICATION
"#;
        write_executable(&live_overlay.join("etc/profile.d/lang.sh"), locale_profile)?;
    }

    // Ensure runtime sshd directory exists on tmpfs-backed /run before sshd starts.
    fs::write(
        live_overlay.join("etc/tmpfiles.d/sshd-local.conf"),
        "d /run/sshd 0755 root root -\n",
    )?;

    if let Some(machine_id) = config.machine_id {
        fs::write(live_overlay.join("etc/machine-id"), machine_id)?;
    }

    // Keep root password empty for live autologin, but avoid "password change
    // required" at first login by using a non-zero lastchg day.
    let shadow_content = "root::20000:0:99999:7:::\n\
                          bin:!:0:0:99999:7:::\n\
                          daemon:!:0:0:99999:7:::\n\
                          nobody:!:0:0:99999:7:::\n";
    fs::write(live_overlay.join("etc/shadow"), shadow_content)?;
    let mut perms = fs::metadata(live_overlay.join("etc/shadow"))?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(live_overlay.join("etc/shadow"), perms)?;
    fs::write(
        live_overlay.join(recinit::live::PAYLOAD_ROLE_FILE),
        format!("{}\n", recinit::live::ROLE_OVERLAY),
    )?;

    println!(
        "  Systemd live overlay created at {}",
        live_overlay.display()
    );
    Ok(live_overlay)
}

/// Write a file and make it executable (mode 0o755).
fn write_executable(path: &Path, content: &str) -> Result<()> {
    fs::write(path, content)?;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

/// Install recqemu's serial test profile, which the install tests' serial
/// console relies on (echo off, empty prompt, readiness marker).
fn write_serial_test_profile(live_overlay: &Path) -> Result<()> {
    let profile_d = live_overlay.join("etc/profile.d");
    fs::create_dir_all(&profile_d)?;
    fs::write(
        profile_d.join("00-serial-test.sh"),
        recqemu::serial::protocol::guest_profile(),
    )?;
    Ok(())
}
