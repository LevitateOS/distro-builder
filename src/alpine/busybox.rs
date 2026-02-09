//! Busybox custom operations.
//!
//! Creates applet symlinks for busybox.

use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

use super::context::BuildContext;

/// Create busybox applet symlinks.
///
/// Busybox provides many utilities through a single binary.
/// Each utility is accessed via a symlink to busybox.
pub fn create_applet_symlinks(ctx: &BuildContext) -> Result<()> {
    let staging = &ctx.staging;

    // Find busybox in staging
    let busybox_path = staging.join("usr/bin/busybox");
    if !busybox_path.exists() {
        // Try to copy from source
        let src = ctx.source.join("bin/busybox");
        if src.exists() {
            fs::create_dir_all(staging.join("usr/bin"))?;
            fs::copy(&src, &busybox_path)?;
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&busybox_path, fs::Permissions::from_mode(0o755))?;
        } else {
            anyhow::bail!("busybox not found in source");
        }
    }

    // Get list of applets from busybox
    let applets = get_busybox_applets(&busybox_path)?;

    // Create symlinks
    let bin_dir = staging.join("usr/bin");
    let sbin_dir = staging.join("usr/sbin");
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&sbin_dir)?;

    for applet in &applets {
        // Determine if this is a sbin command
        let (dir, target) = if is_sbin_applet(applet) {
            (&sbin_dir, "/usr/bin/busybox")
        } else {
            (&bin_dir, "/usr/bin/busybox")
        };

        let link = dir.join(applet);

        // Don't overwrite existing files (might be standalone binaries)
        if !link.exists() && !link.is_symlink() {
            std::os::unix::fs::symlink(target, &link)?;
        }
    }

    // Create essential symlinks in /usr/bin that may be needed
    // Note: /bin is a symlink to /usr/bin (merged-usr), so we put these in usr/bin directly
    // The FHS symlinks are created by FILESYSTEM component before this runs
    {
        let name = "sh";
        let link = bin_dir.join(name);
        if !link.exists() && !link.is_symlink() {
            std::os::unix::fs::symlink("/usr/bin/busybox", &link)?;
        }
    }

    println!("  Created {} busybox applet symlinks", applets.len());

    Ok(())
}

/// Get list of busybox applets.
fn get_busybox_applets(busybox_path: &std::path::Path) -> Result<Vec<String>> {
    // Try running busybox --list
    let output = Command::new(busybox_path)
        .arg("--list")
        .output()
        .context("failed to run busybox --list")?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Ok(stdout
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect());
    }

    // Fallback: use hardcoded list of common applets
    Ok(COMMON_APPLETS.iter().map(|s| s.to_string()).collect())
}

/// Check if an applet should go in /sbin.
fn is_sbin_applet(applet: &str) -> bool {
    SBIN_APPLETS.contains(&applet)
}

/// Applets that belong in /sbin.
const SBIN_APPLETS: &[&str] = &[
    "acpid",
    "adjtimex",
    "blkid",
    "blockdev",
    "bootchartd",
    "depmod",
    "devmem",
    "fdisk",
    "findfs",
    "fsck",
    "fstrim",
    "getty",
    "halt",
    "hdparm",
    "hwclock",
    "ifconfig",
    "ifdown",
    "ifenslave",
    "ifup",
    "init",
    "insmod",
    "ip",
    "ipaddr",
    "iplink",
    "ipneigh",
    "iproute",
    "iprule",
    "iptunnel",
    "klogd",
    "loadfont",
    "loadkmap",
    "logread",
    "losetup",
    "lsmod",
    "makedevs",
    "mdev",
    "mkdosfs",
    "mke2fs",
    "mkfs.ext2",
    "mkfs.fat",
    "mkfs.minix",
    "mkfs.vfat",
    "mkswap",
    "modinfo",
    "modprobe",
    "nameif",
    "nologin",
    "pivot_root",
    "poweroff",
    "reboot",
    "rmmod",
    "route",
    "run-init",
    "setconsole",
    "slattach",
    "start-stop-daemon",
    "sulogin",
    "swapoff",
    "swapon",
    "switch_root",
    "sysctl",
    "syslogd",
    "tunctl",
    "udhcpc",
    "vconfig",
    "watchdog",
    "zcip",
];

/// Common busybox applets (fallback if --list fails).
const COMMON_APPLETS: &[&str] = &[
    // Core utilities
    "ash",
    "sh",
    "cat",
    "chgrp",
    "chmod",
    "chown",
    "cp",
    "date",
    "dd",
    "df",
    "dmesg",
    "echo",
    "false",
    "hostname",
    "kill",
    "ln",
    "login",
    "ls",
    "mkdir",
    "mknod",
    "mount",
    "mv",
    "pidof",
    "ps",
    "pwd",
    "rm",
    "rmdir",
    "sed",
    "sleep",
    "stat",
    "stty",
    "su",
    "sync",
    "true",
    "umount",
    "uname",
    // Extended utilities
    "awk",
    "base64",
    "basename",
    "bunzip2",
    "bzcat",
    "bzip2",
    "cal",
    "chroot",
    "clear",
    "cmp",
    "comm",
    "cut",
    "diff",
    "dirname",
    "du",
    "env",
    "expr",
    "find",
    "fold",
    "free",
    "grep",
    "gzip",
    "head",
    "hexdump",
    "id",
    "install",
    "killall",
    "less",
    "logger",
    "md5sum",
    "mkfifo",
    "mktemp",
    "more",
    "nc",
    "nohup",
    "od",
    "patch",
    "pgrep",
    "ping",
    "ping6",
    "pkill",
    "printf",
    "readlink",
    "realpath",
    "renice",
    "rev",
    "seq",
    "sha1sum",
    "sha256sum",
    "sha512sum",
    "sort",
    "split",
    "strings",
    "tail",
    "tar",
    "tee",
    "test",
    "time",
    "timeout",
    "touch",
    "tr",
    "traceroute",
    "tty",
    "uniq",
    "unlink",
    "unxz",
    "unzip",
    "uptime",
    "vi",
    "watch",
    "wc",
    "wget",
    "which",
    "whoami",
    "xargs",
    "xz",
    "yes",
    "zcat",
    // Init and system
    "init",
    "halt",
    "poweroff",
    "reboot",
    // Networking
    "ifconfig",
    "ip",
    "route",
    "udhcpc",
    "ping",
    // sbin utilities
    "blkid",
    "fdisk",
    "fsck",
    "getty",
    "hwclock",
    "insmod",
    "klogd",
    "loadkmap",
    "losetup",
    "lsmod",
    "mdev",
    "mkfs.ext2",
    "mkfs.vfat",
    "mkswap",
    "modinfo",
    "modprobe",
    "pivot_root",
    "rmmod",
    "swapoff",
    "swapon",
    "switch_root",
    "sysctl",
    "syslogd",
];
