use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use distro_contract::{InstallDocs, InstallUxContract, RuntimeActionContract};

use crate::live_ux::{
    INSTALL_DOCS, INSTALL_DOCS_TEXT, INSTALL_ENTRYPOINT, INSTALL_UX_PROFILE, PROBE_PREFIX,
    SMOKE_ENV, SPLIT_HELPER,
};

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
    distro_id: &str,
    runtime_actions: &[RuntimeActionContract],
) -> Result<()> {
    let tool_payload_dest_dirs =
        tool_payload_destination_dirs(rootfs_source_dir, tool_payload_dir)?;

    for action in runtime_actions {
        match action {
            RuntimeActionContract::ToolPayloadWorkspaceBinary {
                package,
                binary,
                target,
            } => install_tool_payload_workspace_binary(
                repo_root,
                &tool_payload_dest_dirs,
                package,
                binary.as_deref(),
                target.as_deref(),
            )
            .with_context(|| {
                format!(
                    "installing tool payload workspace binary '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    distro_id
                )
            })?,
            RuntimeActionContract::RootfsWorkspaceBinary {
                package,
                binary,
                target,
                destination,
            } => install_rootfs_workspace_binary(
                repo_root,
                rootfs_source_dir,
                package,
                binary.as_deref(),
                target.as_deref(),
                Path::new(destination),
            )
            .with_context(|| {
                format!(
                    "installing rootfs workspace binary '{}' into '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    destination,
                    distro_id
                )
            })?,
            RuntimeActionContract::ApkPackages { packages } => {
                ensure_apk_packages(rootfs_source_dir, packages).with_context(|| {
                    format!(
                        "installing live-tools apk packages for '{}': {}",
                        distro_id,
                        packages.join(", ")
                    )
                })?
            }
        }
    }

    Ok(())
}

fn tool_payload_destination_dirs(
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
) -> Result<Vec<PathBuf>> {
    // Preserve merged-/usr roots (e.g. /bin -> usr/bin): creating a real
    // overlay /bin directory would shadow the symlink and hide /bin/bash,/bin/sh.
    let mut dest_dirs = vec![tool_payload_dir.join("usr/bin")];
    let rootfs_bin = rootfs_source_dir.join("bin");
    if let Ok(meta) = fs::symlink_metadata(&rootfs_bin) {
        if meta.file_type().is_dir() {
            dest_dirs.push(tool_payload_dir.join("bin"));
        }
    }
    for dest_dir in &dest_dirs {
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("creating tool destination '{}'", dest_dir.display()))?;
    }
    Ok(dest_dirs)
}

fn install_tool_payload_workspace_binary(
    repo_root: &Path,
    destination_dirs: &[PathBuf],
    package_name: &str,
    binary_name: Option<&str>,
    target: Option<&str>,
) -> Result<()> {
    let built =
        crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
            .with_context(|| {
                format!(
                    "building workspace binary '{}' from package '{}'",
                    binary_name.unwrap_or(package_name),
                    package_name
                )
            })?;
    let output_name = binary_name.unwrap_or(package_name);

    for destination_dir in destination_dirs {
        copy_executable(&built, &destination_dir.join(output_name)).with_context(|| {
            format!(
                "copying tool payload binary '{}' into '{}'",
                built.display(),
                destination_dir.display()
            )
        })?;
    }

    Ok(())
}

fn install_rootfs_workspace_binary(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    package_name: &str,
    binary_name: Option<&str>,
    target: Option<&str>,
    destination: &Path,
) -> Result<()> {
    let built =
        crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
            .with_context(|| {
                format!(
                    "building workspace binary '{}' from package '{}'",
                    binary_name.unwrap_or(package_name),
                    package_name
                )
            })?;
    let destination = rootfs_source_dir.join(destination);
    copy_executable(&built, &destination).with_context(|| {
        format!(
            "copying rootfs workspace binary '{}' -> '{}'",
            built.display(),
            destination.display()
        )
    })
}

fn copy_executable(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating destination dir '{}'", parent.display()))?;
    }
    fs::copy(source, destination).with_context(|| {
        format!(
            "copying executable from '{}' to '{}'",
            source.display(),
            destination.display()
        )
    })?;
    let mut perms = fs::metadata(destination)
        .with_context(|| format!("reading metadata for '{}'", destination.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(destination, perms).with_context(|| {
        format!(
            "setting executable permissions on '{}'",
            destination.display()
        )
    })?;
    Ok(())
}

fn ensure_apk_packages(rootfs_source_dir: &Path, packages: &[String]) -> Result<()> {
    if packages.is_empty() {
        return Ok(());
    }

    let resolv_conf = rootfs_source_dir.join("etc/resolv.conf");
    if let Some(parent) = resolv_conf.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::copy("/etc/resolv.conf", &resolv_conf).with_context(|| {
        format!(
            "copying host resolv.conf into prepared rootfs '{}'",
            resolv_conf.display()
        )
    })?;

    let pkg_string = packages.join(" ");
    let output = Command::new("unshare")
        .arg("-Urpf")
        .arg("/bin/sh")
        .arg("-c")
        .arg("chroot \"$1\" /bin/sh -lc \"$2\"")
        .arg("_")
        .arg(rootfs_source_dir)
        .arg(format!(
            "apk add --no-cache --no-check-certificate {}",
            pkg_string
        ))
        .output()
        .with_context(|| {
            format!(
                "running apk package install in rootfs '{}'",
                rootfs_source_dir.display()
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "apk package install failed for packages [{}]: {}\n{}",
        pkg_string,
        stdout.trim(),
        stderr.trim()
    )
}

fn find_executable_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|entry| entry.join(name))
        .find(|candidate| candidate.is_file())
}

fn install_bun_docs_tui_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    app: &str,
    docs_cmd_path: &Path,
) -> Result<()> {
    let docs_app = distro_contract::tui_apps::find(repo_root, app)?;

    let bun_bin = find_executable_in_path("bun").ok_or_else(|| {
        anyhow::anyhow!(
            "bun is required to build the live install docs payload.\n\
             Remediation: install bun and ensure it is in PATH (https://bun.sh)."
        )
    })?;

    // The app is a member of the root JavaScript workspace; install it the one way xtask does.
    let install = Command::new(&bun_bin)
        .args(["install", "--frozen-lockfile"])
        .current_dir(repo_root)
        .output()
        .context("running bun install --frozen-lockfile at the repo root")?;
    if !install.status.success() {
        bail!(
            "bun install --frozen-lockfile failed at '{}': {}\n{}",
            repo_root.display(),
            String::from_utf8_lossy(&install.stdout).trim(),
            String::from_utf8_lossy(&install.stderr).trim()
        );
    }

    let docs_bundle_dir = rootfs_source_dir.join("usr/local/share/levitate/docs-tui");
    fs::create_dir_all(&docs_bundle_dir).with_context(|| {
        format!(
            "creating live install docs bundle directory '{}'",
            docs_bundle_dir.display()
        )
    })?;
    let docs_bundle = docs_bundle_dir.join("levitate-install-docs.js");

    let build = Command::new(&bun_bin)
        .arg("build")
        .arg(&docs_app.entry)
        .arg("--target=bun")
        .arg("--minify")
        .arg("--outfile")
        .arg(&docs_bundle)
        .current_dir(&docs_app.dir)
        .output()
        .with_context(|| {
            format!(
                "building live install docs bundle from '{}' to '{}'",
                docs_app.dir.display(),
                docs_bundle.display()
            )
        })?;
    if !build.status.success() {
        let stdout = String::from_utf8_lossy(&build.stdout);
        let stderr = String::from_utf8_lossy(&build.stderr);
        bail!(
            "bun build failed for live install docs bundle '{}': {}\n{}",
            docs_app.dir.display(),
            stdout.trim(),
            stderr.trim()
        );
    }
    if !docs_bundle.is_file() {
        bail!(
            "live install docs bundle missing after build at '{}'",
            docs_bundle.display()
        );
    }

    let bun_dest = rootfs_source_dir.join("usr/local/bin/bun");
    if let Some(parent) = bun_dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating bun destination '{}'", parent.display()))?;
    }
    fs::copy(&bun_bin, &bun_dest).with_context(|| {
        format!(
            "copying bun runtime '{}' -> '{}'",
            bun_bin.display(),
            bun_dest.display()
        )
    })?;
    let mut bun_perms = fs::metadata(&bun_dest)
        .with_context(|| format!("reading permissions for '{}'", bun_dest.display()))?
        .permissions();
    bun_perms.set_mode(0o755);
    fs::set_permissions(&bun_dest, bun_perms)
        .with_context(|| format!("setting executable permissions on '{}'", bun_dest.display()))?;

    let docs_cmd = crate::guest::render(
        crate::guest::DOCS_BUN,
        &[
            (
                "DOCS_BUNDLE",
                "/usr/local/share/levitate/docs-tui/levitate-install-docs.js",
            ),
            ("BUN", "/usr/local/bin/bun"),
        ],
    )?;
    write_executable(docs_cmd_path, &docs_cmd).with_context(|| {
        format!(
            "installing install docs command launcher '{}'",
            docs_cmd_path.display()
        )
    })?;

    Ok(())
}

/// Writes the install screen the variant declares (`[install_ux]`): the docs, the
/// entrypoint and the login hook that starts it.
pub(crate) fn install_ux_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_ux: &InstallUxContract,
) -> Result<()> {
    let interactive_shell = install_ux.shell.as_str();
    install_shell_color_profile(rootfs_source_dir).with_context(|| {
        format!(
            "installing shell color profile defaults under '{}'",
            rootfs_source_dir.display()
        )
    })?;

    let docs_cmd_path = rootfs_source_dir.join(INSTALL_DOCS);
    let docs_tui_cmd_path = rootfs_source_dir.join("usr/local/bin/docs-tui");
    match &install_ux.docs {
        InstallDocs::TuiApp(app) => {
            install_bun_docs_tui_payload(repo_root, rootfs_source_dir, app, &docs_cmd_path)
                .with_context(|| {
                    format!(
                        "installing bun-based install docs payload for '{}'",
                        distro_id
                    )
                })?
        }
        InstallDocs::Text(guide) => {
            let docs_text_path = rootfs_source_dir.join(INSTALL_DOCS_TEXT);
            let guide_text = fs::read_to_string(guide).with_context(|| {
                format!(
                    "reading the variant install guide '{}' (install_ux.docs.text)",
                    guide.display()
                )
            })?;
            write_text(&docs_text_path, &guide_text).with_context(|| {
                format!(
                    "writing install guide payload '{}'",
                    docs_text_path.display()
                )
            })?;
            let docs_cmd = crate::guest::render(
                crate::guest::DOCS_TEXT,
                &[
                    ("DOCS_TEXT", &format!("/{INSTALL_DOCS_TEXT}")),
                    ("SHELL", interactive_shell),
                ],
            )?;
            write_executable(&docs_cmd_path, &docs_cmd).with_context(|| {
                format!(
                    "installing install docs command '{}'",
                    docs_cmd_path.display()
                )
            })?;
        }
    }

    // Canonical docs-tui command alias for split-pane right-side launchers.
    let docs_tui_cmd = crate::guest::render(
        crate::guest::DOCS_TUI_ALIAS,
        &[("INSTALL_DOCS", &format!("/{INSTALL_DOCS}"))],
    )?;
    write_executable(&docs_tui_cmd_path, &docs_tui_cmd).with_context(|| {
        format!(
            "installing install docs-tui alias '{}'",
            docs_tui_cmd_path.display()
        )
    })?;

    let entrypoint_path = rootfs_source_dir.join(INSTALL_ENTRYPOINT);
    let entrypoint_script = crate::guest::render(
        crate::guest::ENTRYPOINT_UX,
        &[
            ("SHELL", interactive_shell),
            ("DISTRO", distro_id),
            ("SPLIT", SPLIT_HELPER),
            ("PROBE", PROBE_PREFIX),
            ("SMOKE", SMOKE_ENV),
        ],
    )?;
    write_executable(&entrypoint_path, &entrypoint_script).with_context(|| {
        format!(
            "installing install entrypoint script '{}'",
            entrypoint_path.display()
        )
    })?;
    let ux_profile_path = rootfs_source_dir.join(INSTALL_UX_PROFILE);
    let ux_profile = crate::guest::render_boot(crate::guest::INSTALL_UX_PROFILE_HOOK)?;
    write_text(&ux_profile_path, &ux_profile).with_context(|| {
        format!(
            "installing install UX profile hook '{}'",
            ux_profile_path.display()
        )
    })?;

    Ok(())
}

fn install_shell_color_profile(rootfs_source_dir: &Path) -> Result<()> {
    let profile_path = rootfs_source_dir.join("etc/profile.d/25-shell-color.sh");
    let profile = crate::guest::SHELL_COLOR_PROFILE;
    write_text(&profile_path, profile)
        .with_context(|| format!("writing shell color profile '{}'", profile_path.display()))
}

fn write_executable(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    let mut perms = fs::metadata(path)
        .with_context(|| format!("reading metadata '{}'", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("setting executable permissions on '{}'", path.display()))?;
    Ok(())
}

fn write_text(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("writing '{}'", path.display()))?;
    Ok(())
}
