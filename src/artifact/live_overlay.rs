//! Shared live overlay builders.
//!
//! This module provides:
//! - OpenRC live overlay generation (AcornOS, IuppiterOS style)
//! - Systemd live overlay generation (LevitateOS, RalphOS style)

use anyhow::{Context, Result};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::copy_dir_recursive;

/// Inittab variant controlling which consoles are enabled.
#[derive(Debug, Clone, Copy)]
pub enum InittabVariant {
    /// Desktop: 6 VTYs + serial console with autologin (AcornOS).
    DesktopWithSerial,
    /// Headless appliance: serial console only (IuppiterOS).
    SerialOnly,
}

/// Configuration for creating an OpenRC live overlay.
#[derive(Debug)]
pub struct LiveOverlayConfig<'a> {
    /// OS display name (e.g., "AcornOS", "IuppiterOS").
    pub os_name: &'a str,
    /// Which inittab variant to generate.
    pub inittab: InittabVariant,
    /// Optional path to profile/live-overlay directory to copy first.
    pub profile_overlay: Option<&'a Path>,
}

/// Configuration for creating a systemd live overlay.
#[derive(Debug)]
pub struct SystemdLiveOverlayConfig<'a> {
    /// OS display name (e.g., "LevitateOS", "RalphOS").
    pub os_name: &'a str,
    /// Optional override for `/etc/issue`.
    pub issue_message: Option<&'a str>,
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

    // Step 1: Copy profile/live-overlay (test instrumentation, etc.)
    if let Some(profile) = config.profile_overlay {
        if profile.exists() {
            println!("  Copying profile/live-overlay (test instrumentation)...");
            copy_dir_recursive(profile, &live_overlay).with_context(|| {
                format!(
                    "Failed to copy {} -> {}",
                    profile.display(),
                    live_overlay.display()
                )
            })?;
        }
    }

    // Step 2: Code-generated overlay files
    fs::create_dir_all(live_overlay.join("etc")).with_context(|| "Failed to create etc")?;

    // Serial autologin script
    fs::create_dir_all(live_overlay.join("usr/local/bin"))?;
    let autologin_script = r#"#!/bin/sh
# Autologin for serial console testing
# Called by agetty -l as the login program
# agetty has already set up stdin/stdout/stderr on the tty

echo "[autologin] Starting login shell..."

# Run sh as login shell (sources /etc/profile and /etc/profile.d/*)
# In Alpine, /bin/sh is busybox ash
exec /bin/sh -l
"#;
    write_executable(
        &live_overlay.join("usr/local/bin/serial-autologin"),
        autologin_script,
    )?;

    // /etc/issue
    fs::write(
        live_overlay.join("etc/issue"),
        format!(
            "\n{} Live - \\l\n\nLogin as 'root' (no password)\n\n",
            config.os_name
        ),
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
::wait:/sbin/openrc default

# Virtual terminals
tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6

# Serial console with autologin for test harness
# Uses wrapper script that spawns ash as login shell (sources /etc/profile.d/*)
ttyS0::respawn:/sbin/getty -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100

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
::wait:/sbin/openrc default

# Serial console PRIMARY with autologin (ttyS0) - appliance has no display
# Uses wrapper script that spawns ash as login shell (sources /etc/profile.d/*)
ttyS0::respawn:/sbin/getty -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100

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

    let volatile_log_script = r#"#!/bin/sh
# Ensure volatile log storage for live session
# This runs early in boot to catch any logs before syslog starts

# Only mount if not already a tmpfs (idempotent)
if ! mountpoint -q /var/log 2>/dev/null; then
    # Preserve any existing logs created before mount
    if [ -d /var/log ]; then
        mkdir -p /tmp/log-backup
        cp -a /var/log/* /tmp/log-backup/ 2>/dev/null || true
    fi

    mount -t tmpfs -o nosuid,nodev,noexec,size=64M,mode=0755 tmpfs /var/log

    # Restore preserved logs
    if [ -d /tmp/log-backup ]; then
        cp -a /tmp/log-backup/* /var/log/ 2>/dev/null || true
        rm -rf /tmp/log-backup
    fi

    # Ensure log directories exist
    mkdir -p /var/log/chrony 2>/dev/null || true
fi
"#;
    write_executable(
        &live_overlay.join("etc/local.d/00-volatile-log.start"),
        volatile_log_script,
    )?;

    let efivars_script = r#"#!/bin/sh
# Ensure efivarfs is mounted for UEFI support
# Needed for efibootmgr, bootctl, and install tests

if [ -d /sys/firmware/efi ]; then
    mkdir -p /sys/firmware/efi/efivars 2>/dev/null
    mount -t efivarfs efivarfs /sys/firmware/efi/efivars 2>/dev/null || true
fi
"#;
    write_executable(
        &live_overlay.join("etc/local.d/01-efivarfs.start"),
        efivars_script,
    )?;

    // Do-not-suspend configuration
    // Method 1: ACPI handler
    fs::create_dir_all(live_overlay.join("etc/acpi"))?;
    let acpi_handler = format!(
        r#"#!/bin/sh
# {} Live: Disable suspend actions
# Power button and lid close do nothing during live session

case "$1" in
    button/power)
        # Log but don't suspend - user is probably installing
        logger "{} Live: Power button pressed (suspend disabled)"
        ;;
    button/lid)
        # Lid close does nothing - prevent accidental suspend
        logger "{} Live: Lid event ignored (suspend disabled)"
        ;;
    *)
        # Let other events through to default handler
        ;;
esac
"#,
        config.os_name, config.os_name, config.os_name
    );
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
    fs::create_dir_all(live_overlay.join("etc/systemd/system/serial-getty@.service.d"))?;

    let tty1_autologin =
        "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noclear %I $TERM\n";
    fs::write(
        live_overlay.join("etc/systemd/system/getty@tty1.service.d/autologin.conf"),
        tty1_autologin,
    )?;

    let serial_autologin = "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root -L --keep-baud 115200,57600,38400,9600 %I vt100\n";
    fs::write(
        live_overlay.join("etc/systemd/system/serial-getty@.service.d/zz-autologin.conf"),
        serial_autologin,
    )?;

    let default_issue = format!(
        "\n{} Live - \\l\n\nLogin as 'root' (no password)\n\n",
        config.os_name
    );
    fs::write(
        live_overlay.join("etc/issue"),
        config.issue_message.unwrap_or(&default_issue),
    )?;

    let shadow_content = "root::0:0:99999:7:::\n\
                          bin:!:0:0:99999:7:::\n\
                          daemon:!:0:0:99999:7:::\n\
                          nobody:!:0:0:99999:7:::\n";
    fs::write(live_overlay.join("etc/shadow"), shadow_content)?;
    let mut perms = fs::metadata(live_overlay.join("etc/shadow"))?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(live_overlay.join("etc/shadow"), perms)?;

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
