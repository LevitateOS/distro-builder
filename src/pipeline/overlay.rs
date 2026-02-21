use anyhow::{bail, Context, Result};
use std::fs;
use std::os::unix::fs::symlink;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::pipeline::io::rename_live_overlay_for_stage;
use crate::{
    create_openrc_live_overlay, create_systemd_live_overlay, InittabVariant, LiveOverlayConfig,
    SystemdLiveOverlayConfig,
};

const STAGE_MACHINE_ID: &str = "0123456789abcdef0123456789abcdef\n";
#[derive(Debug, Clone)]
pub enum S01OverlayPolicy {
    Systemd {
        issue_message: Option<String>,
    },
    OpenRc {
        inittab: InittabVariant,
        profile_overlay: Option<PathBuf>,
    },
}

pub(crate) fn create_live_overlay(
    output_dir: &Path,
    distro_id: &str,
    os_name: &str,
    stage_label: &str,
    artifact_tag: &str,
    overlay: &S01OverlayPolicy,
) -> Result<PathBuf> {
    let stage_issue_banner = stage_issue_banner(os_name, stage_label);
    let live_overlay_dir = match overlay {
        S01OverlayPolicy::Systemd { issue_message } => create_systemd_live_overlay(
            output_dir,
            &SystemdLiveOverlayConfig {
                os_name,
                issue_message: issue_message
                    .as_deref()
                    .or(Some(stage_issue_banner.as_str())),
                masked_units: &[],
                write_serial_test_profile: true,
                machine_id: Some(STAGE_MACHINE_ID),
                enforce_utf8_locale_profile: false,
            },
        )
        .with_context(|| format!("creating systemd live overlay for {}", distro_id))?,
        S01OverlayPolicy::OpenRc {
            inittab,
            profile_overlay,
        } => create_openrc_live_overlay(
            output_dir,
            &LiveOverlayConfig {
                os_name,
                inittab: *inittab,
                profile_overlay: profile_overlay.as_deref(),
                issue_message: Some(stage_issue_banner.as_str()),
            },
        )
        .with_context(|| format!("creating openrc live overlay for {}", distro_id))?,
    };

    rename_live_overlay_for_stage(output_dir, &live_overlay_dir, artifact_tag).with_context(|| {
        format!(
            "renaming {} live overlay directory for '{}'",
            stage_label, distro_id
        )
    })
}

pub(crate) fn ensure_systemd_default_target(rootfs_dir: &Path) -> Result<()> {
    let default_target = rootfs_dir.join("etc/systemd/system/default.target");
    if let Some(parent) = default_target.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    if default_target.exists() || default_target.symlink_metadata().is_ok() {
        fs::remove_file(&default_target)
            .with_context(|| format!("removing '{}'", default_target.display()))?;
    }
    symlink("/usr/lib/systemd/system/multi-user.target", &default_target).with_context(|| {
        format!(
            "linking '{}' -> '/usr/lib/systemd/system/multi-user.target'",
            default_target.display()
        )
    })?;
    Ok(())
}

pub(crate) fn ensure_systemd_sshd_dirs(rootfs_dir: &Path) -> Result<()> {
    for rel in ["var/empty/sshd", "usr/share/empty.sshd"] {
        let privsep_dir = rootfs_dir.join(rel);
        fs::create_dir_all(&privsep_dir)
            .with_context(|| format!("creating '{}'", privsep_dir.display()))?;
        let mut perms = fs::metadata(&privsep_dir)
            .with_context(|| format!("reading metadata '{}'", privsep_dir.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&privsep_dir, perms)
            .with_context(|| format!("setting permissions '{}'", privsep_dir.display()))?;
    }

    let ssh_dir = rootfs_dir.join("etc/ssh");
    let sshd_config = ssh_dir.join("sshd_config");
    if !sshd_config.is_file() {
        let anaconda_config = ssh_dir.join("sshd_config.anaconda");
        if anaconda_config.is_file() {
            fs::copy(&anaconda_config, &sshd_config).with_context(|| {
                format!(
                    "copying fallback sshd config '{}' -> '{}'",
                    anaconda_config.display(),
                    sshd_config.display()
                )
            })?;
        } else {
            fs::create_dir_all(&ssh_dir)
                .with_context(|| format!("creating '{}'", ssh_dir.display()))?;
            fs::write(
                &sshd_config,
                "PermitRootLogin yes\nPasswordAuthentication yes\nUsePAM yes\nInclude /etc/ssh/sshd_config.d/*.conf\n",
            )
            .with_context(|| format!("writing fallback sshd config '{}'", sshd_config.display()))?;
        }
    }
    Ok(())
}

pub(crate) fn ensure_systemd_locale_completeness(rootfs_dir: &Path) -> Result<()> {
    let locale_payload_candidates = [
        "lib/locale/C.utf8/LC_CTYPE",
        "usr/lib/locale/C.utf8/LC_CTYPE",
        "lib64/locale/C.utf8/LC_CTYPE",
        "usr/lib64/locale/C.utf8/LC_CTYPE",
    ];
    let has_utf8_payload = locale_payload_candidates
        .iter()
        .any(|rel| rootfs_dir.join(rel).is_file());
    if !has_utf8_payload {
        bail!(
            "missing UTF-8 locale payload in Stage systemd rootfs '{}'; expected one of: {}",
            rootfs_dir.display(),
            locale_payload_candidates.join(", ")
        );
    }

    let lib_locale = rootfs_dir.join("lib/locale");
    let usr_lib_locale = rootfs_dir.join("usr/lib/locale");
    if lib_locale.is_dir() && !usr_lib_locale.exists() {
        if let Some(parent) = usr_lib_locale.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating '{}'", parent.display()))?;
        }
        symlink("/lib/locale", &usr_lib_locale)
            .with_context(|| format!("linking '{}' -> '/lib/locale'", usr_lib_locale.display()))?;
    }

    let etc_dir = rootfs_dir.join("etc");
    fs::create_dir_all(&etc_dir).with_context(|| format!("creating '{}'", etc_dir.display()))?;
    fs::write(etc_dir.join("locale.conf"), "LANG=C.UTF-8\n").with_context(|| {
        format!(
            "writing canonical locale config '{}'",
            etc_dir.join("locale.conf").display()
        )
    })?;
    Ok(())
}

pub(crate) fn ensure_required_service_wiring(
    live_overlay_dir: &Path,
    overlay_policy: &S01OverlayPolicy,
    required_services: &[String],
) -> Result<()> {
    for service in required_services {
        match (overlay_policy, service.as_str()) {
            (S01OverlayPolicy::Systemd { .. }, "sshd") => {
                let wants_dir = live_overlay_dir.join("etc/systemd/system/multi-user.target.wants");
                fs::create_dir_all(&wants_dir)
                    .with_context(|| format!("creating '{}'", wants_dir.display()))?;
                let wants_link = wants_dir.join("sshd.service");
                if wants_link.symlink_metadata().is_ok() {
                    fs::remove_file(&wants_link)
                        .with_context(|| format!("removing '{}'", wants_link.display()))?;
                }
                symlink("/usr/lib/systemd/system/sshd.service", &wants_link).with_context(
                    || {
                        format!(
                            "linking '{}' -> '/usr/lib/systemd/system/sshd.service'",
                            wants_link.display()
                        )
                    },
                )?;
            }
            (S01OverlayPolicy::OpenRc { .. }, "sshd") => {
                let runlevel_dir = live_overlay_dir.join("etc/runlevels/default");
                fs::create_dir_all(&runlevel_dir)
                    .with_context(|| format!("creating '{}'", runlevel_dir.display()))?;
                let service_link = runlevel_dir.join("sshd");
                if service_link.symlink_metadata().is_ok() {
                    fs::remove_file(&service_link)
                        .with_context(|| format!("removing '{}'", service_link.display()))?;
                }
                symlink("/etc/init.d/sshd", &service_link).with_context(|| {
                    format!("linking '{}' -> '/etc/init.d/sshd'", service_link.display())
                })?;
            }
            (_, other) => {
                bail!("unsupported Stage 01 required service '{}'", other);
            }
        }
    }
    Ok(())
}

pub(crate) fn ensure_openrc_shell(
    rootfs_source_dir: &Path,
    os_name: &str,
    inittab: InittabVariant,
) -> Result<()> {
    let etc_dir = rootfs_source_dir.join("etc");
    let usr_local_bin = rootfs_source_dir.join("usr/local/bin");
    fs::create_dir_all(&etc_dir)
        .with_context(|| format!("creating OpenRC etc dir '{}'", etc_dir.display()))?;
    fs::create_dir_all(&usr_local_bin).with_context(|| {
        format!(
            "creating OpenRC usr/local/bin dir '{}'",
            usr_local_bin.display()
        )
    })?;

    let autologin = usr_local_bin.join("serial-autologin");
    fs::write(
        &autologin,
        "#!/bin/sh\necho \"___SHELL_READY___\"\necho \"___SHELL_READY___\" >/dev/console 2>/dev/null || true\necho \"___SHELL_READY___\" >/dev/kmsg 2>/dev/null || true\nexec /bin/sh -l\n",
    )
    .with_context(|| format!("writing '{}'", autologin.display()))?;
    let mut perms = fs::metadata(&autologin)
        .with_context(|| format!("reading metadata '{}'", autologin.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&autologin, perms)
        .with_context(|| format!("setting permissions on '{}'", autologin.display()))?;

    let inittab_content = match inittab {
        InittabVariant::DesktopWithSerial => format!(
            r#"# /etc/inittab - {os_name} Live
# Stage 01 boots to minimal interactive shell.
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
tty1::respawn:/sbin/getty 38400 tty1
tty2::respawn:/sbin/getty 38400 tty2
tty3::respawn:/sbin/getty 38400 tty3
tty4::respawn:/sbin/getty 38400 tty4
tty5::respawn:/sbin/getty 38400 tty5
tty6::respawn:/sbin/getty 38400 tty6
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100
::wait:/sbin/openrc default
::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
"#
        ),
        InittabVariant::SerialOnly => format!(
            r#"# /etc/inittab - {os_name} Live
# Stage 01 boots to minimal interactive shell.
::sysinit:/sbin/openrc sysinit
::sysinit:/sbin/openrc boot
ttyS0::respawn:/sbin/getty -L -n -l /usr/local/bin/serial-autologin 115200 ttyS0 vt100
::wait:/sbin/openrc default
::ctrlaltdel:/sbin/reboot
::shutdown:/sbin/openrc shutdown
"#
        ),
    };
    let inittab_path = etc_dir.join("inittab");
    fs::write(&inittab_path, inittab_content)
        .with_context(|| format!("writing '{}'", inittab_path.display()))?;

    let issue_path = etc_dir.join("issue");
    fs::write(
        &issue_path,
        format!(
            "\n{} S01 Boot Live - \\l\n\nLogin as 'root' (no password)\n\n",
            os_name
        ),
    )
    .with_context(|| format!("writing '{}'", issue_path.display()))?;

    let shadow_path = etc_dir.join("shadow");
    fs::write(
        &shadow_path,
        "root::0:0:99999:7:::\nbin:!:0:0:99999:7:::\ndaemon:!:0:0:99999:7:::\nnobody:!:0:0:99999:7:::\n",
    )
    .with_context(|| format!("writing '{}'", shadow_path.display()))?;
    let mut shadow_perms = fs::metadata(&shadow_path)
        .with_context(|| format!("reading metadata '{}'", shadow_path.display()))?
        .permissions();
    shadow_perms.set_mode(0o640);
    fs::set_permissions(&shadow_path, shadow_perms)
        .with_context(|| format!("setting permissions on '{}'", shadow_path.display()))?;

    Ok(())
}

fn stage_issue_banner(os_name: &str, stage_label: &str) -> String {
    format!(
        "\n{} {} Live - \\l\n\nLogin as 'root' (no password)\n\n",
        os_name, stage_label
    )
}
