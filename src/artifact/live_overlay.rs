//! Shared live overlay builders.
//!
//! This module provides:
//! - OpenRC live overlay generation (AcornOS, IuppiterOS style)
//! - Systemd live overlay generation (LevitateOS, RalphOS style)

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
    /// Optional override for `/etc/issue`.
    pub issue_message: Option<&'a str>,
}

/// Configuration for creating a systemd live overlay.
#[derive(Debug)]
pub struct SystemdLiveOverlayConfig<'a> {
    /// OS display name (e.g., "LevitateOS", "RalphOS").
    pub os_name: &'a str,
    /// Optional override for `/etc/issue`.
    pub issue_message: Option<&'a str>,
    /// Systemd unit names to mask by linking to `/dev/null`.
    pub masked_units: &'a [&'a str],
    /// Whether to write serial-console stage test marker profile script.
    pub write_serial_test_profile: bool,
    /// Optional machine-id content to write at `/etc/machine-id`.
    pub machine_id: Option<&'a str>,
    /// When true, install a strict UTF-8 locale profile for live shells.
    /// Stage producers must ensure UTF-8 locale payload exists in rootfs.
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
echo "___SHELL_READY___"
echo "[autologin] Starting login shell..." >/dev/console 2>/dev/null || true
echo "___SHELL_READY___" >/dev/console 2>/dev/null || true
echo "___SHELL_READY___" >/dev/kmsg 2>/dev/null || true

# Run sh as login shell (sources /etc/profile and /etc/profile.d/*)
# In Alpine, /bin/sh is busybox ash
exec /bin/sh -l
"#;
    write_executable(
        &live_overlay.join("usr/local/bin/serial-autologin"),
        autologin_script,
    )?;

    // /etc/issue
    let default_issue = format!(
        "\n{} Live - \\l\n\nLogin as 'root' (no password)\n\n",
        config.os_name
    );
    fs::write(
        live_overlay.join("etc/issue"),
        config.issue_message.unwrap_or(&default_issue),
    )?;
    fs::write(live_overlay.join(".live-payload-role"), "overlay\n")?;

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

    let tty1_autologin =
        "[Service]\nExecStart=\nExecStart=-/sbin/agetty --autologin root --noclear %I $TERM\n";
    fs::write(
        live_overlay.join("etc/systemd/system/getty@tty1.service.d/autologin.conf"),
        tty1_autologin,
    )?;

    let serial_autologin_script = r#"#!/bin/sh
echo "___SHELL_READY___"
    # Stage live override: use portable C locale (always available in minimal rootfs).
    export LANG=C
unset LC_ALL LC_CTYPE LC_NUMERIC LC_TIME LC_COLLATE LC_MESSAGES \
      LC_MONETARY LC_PAPER LC_NAME LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT \
      LC_IDENTIFICATION
exec /bin/bash -il
"#;
    write_executable(
        &live_overlay.join("usr/local/bin/serial-autologin"),
        serial_autologin_script,
    )?;

    let serial_autologin = "[Service]\nExecStart=\nExecStart=-/sbin/agetty -n -l /usr/local/bin/serial-autologin 115200,57600,38400,9600 %I vt100\n";
    fs::write(
        live_overlay.join("etc/systemd/system/serial-getty@.service.d/zz-autologin.conf"),
        serial_autologin,
    )?;
    fs::write(
        live_overlay.join("etc/systemd/system/getty@.service.d/zz-autologin.conf"),
        serial_autologin,
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
    let live_net_setup_script = r#"#!/bin/sh
set -eu

NIC=""
for dev in /sys/class/net/*; do
    [ -e "$dev" ] || continue
    name="$(basename "$dev")"
    [ "$name" = "lo" ] && continue
    NIC="$name"
    break
done

[ -n "$NIC" ] || exit 1

/usr/sbin/ip link set "$NIC" up
/usr/sbin/ip addr add 10.0.2.15/24 dev "$NIC" 2>/dev/null || true
/usr/sbin/ip route replace default via 10.0.2.2 dev "$NIC"
"#;
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

    let shutdown_cleanup_script = r#"#!/bin/sh
# Live ISO shutdown cleanup:
# release loop-backed live mounts before systemd reaches umount.target.
set +e

for mp in /live-overlay /rootfs /run/live-media; do
    if mountpoint -q "$mp"; then
        umount "$mp" >/dev/null 2>&1 || umount -l "$mp" >/dev/null 2>&1
    fi
done

if [ -d /run/live-media ]; then
    rmdir /run/live-media 2>/dev/null || true
fi

for loopdev in /dev/loop1 /dev/loop0; do
    if [ -b "$loopdev" ]; then
        losetup -d "$loopdev" >/dev/null 2>&1 || true
    fi
done

if mountpoint -q /media/cdrom; then
    umount /media/cdrom >/dev/null 2>&1 || umount -l /media/cdrom >/dev/null 2>&1
fi

exit 0
"#;
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

    let default_issue = format!(
        "\n{} Live - \\l\n\nLogin as 'root' (no password)\n\n",
        config.os_name
    );
    fs::write(
        live_overlay.join("etc/issue"),
        config.issue_message.unwrap_or(&default_issue),
    )?;

    if config.write_serial_test_profile {
        let test_profile = r#"# Stage harness markers (serial console only)
case "$-" in
    *i*) ;;
    *) return 0 ;;
esac

if [ "$(tty 2>/dev/null)" = "/dev/ttyS0" ]; then
    echo "___SHELL_READY___"
fi
"#;
        fs::write(
            live_overlay.join("etc/profile.d/00-live-test.sh"),
            test_profile,
        )?;
    }

    if config.enforce_utf8_locale_profile {
        // Stage live override: use portable C locale for minimal rootfs payloads.
        let locale_profile = r#"#!/bin/sh
# Stage live override: use portable C locale (always available).
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
    fs::write(live_overlay.join(".live-payload-role"), "overlay\n")?;

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
