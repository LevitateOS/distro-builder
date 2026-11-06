use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) use distro_contract::{InstallDocsFrontend, InstallExperience};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LiveToolsRuntimeAction {
    ToolPayloadWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
    },
    RootfsWorkspaceBinary {
        package: String,
        binary: Option<String>,
        target: Option<String>,
        destination: PathBuf,
    },
    ApkPackages {
        packages: Vec<String>,
    },
    InstallModePayload {
        interactive_shell: String,
        ux_docs_frontend: InstallDocsFrontend,
    },
}

use crate::live_ux::{
    INSTALL_DOCS, INSTALL_DOCS_TEXT, INSTALL_ENTRYPOINT, INSTALL_EXPERIENCE_MARKER,
    INSTALL_UX_PROFILE, PROBE_PREFIX, SMOKE_ENV, SPLIT_HELPER,
};

pub(crate) fn add_required_tools(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    tool_payload_dir: &Path,
    distro_id: &str,
    install_experience: InstallExperience,
    runtime_actions: &[LiveToolsRuntimeAction],
) -> Result<()> {
    let tool_payload_dest_dirs =
        tool_payload_destination_dirs(rootfs_source_dir, tool_payload_dir)?;

    for action in runtime_actions {
        match action {
            LiveToolsRuntimeAction::ToolPayloadWorkspaceBinary {
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
            LiveToolsRuntimeAction::RootfsWorkspaceBinary {
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
                destination,
            )
            .with_context(|| {
                format!(
                    "installing rootfs workspace binary '{}' into '{}' for '{}'",
                    binary.as_deref().unwrap_or(package),
                    destination.display(),
                    distro_id
                )
            })?,
            LiveToolsRuntimeAction::ApkPackages { packages } => {
                ensure_apk_packages(rootfs_source_dir, packages).with_context(|| {
                    format!(
                        "installing live-tools apk packages for '{}': {}",
                        distro_id,
                        packages.join(", ")
                    )
                })?
            }
            LiveToolsRuntimeAction::InstallModePayload {
                interactive_shell,
                ux_docs_frontend,
            } => install_mode_payload(
                repo_root,
                rootfs_source_dir,
                distro_id,
                install_experience,
                interactive_shell,
                ux_docs_frontend,
            )
            .with_context(|| format!("writing install experience payload for '{}'", distro_id))?,
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
    let built = crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
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
    let built = crate::recipe::tools::build_tool_program(repo_root, package_name, binary_name, target)
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
    docs_cmd_path: &Path,
) -> Result<()> {
    let docs_app = distro_contract::tui_apps::find(repo_root, "install-docs")?;

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

    let docs_cmd = "#!/bin/sh\n\
set -eu\n\
\n\
APP=\"/usr/local/share/levitate/docs-tui/levitate-install-docs.js\"\n\
if [ ! -r \"$APP\" ]; then\n\
    echo \"install docs bundle missing at $APP\" >&2\n\
    exit 1\n\
fi\n\
\n\
case \"${TERM:-}\" in\n\
    \"\"|dumb|vt100|vt102|linux)\n\
        export TERM=xterm-256color\n\
        ;;\n\
esac\n\
export COLORTERM=\"${COLORTERM:-truecolor}\"\n\
export FORCE_COLOR=\"${FORCE_COLOR:-3}\"\n\
if [ -n \"${NO_COLOR:-}\" ]; then\n\
    unset NO_COLOR\n\
fi\n\
\n\
exec /usr/local/bin/bun \"$APP\" \"$@\"\n";
    write_executable(docs_cmd_path, docs_cmd).with_context(|| {
        format!(
            "installing install docs command launcher '{}'",
            docs_cmd_path.display()
        )
    })?;

    Ok(())
}

fn install_mode_payload(
    repo_root: &Path,
    rootfs_source_dir: &Path,
    distro_id: &str,
    install_experience: InstallExperience,
    interactive_shell: &str,
    ux_docs_frontend: &InstallDocsFrontend,
) -> Result<()> {
    let marker_path = rootfs_source_dir.join(INSTALL_EXPERIENCE_MARKER);
    write_text(&marker_path, &format!("{}\n", install_experience.as_str())).with_context(|| {
        format!(
            "writing install experience marker '{}'",
            marker_path.display()
        )
    })?;
    install_shell_color_profile(rootfs_source_dir).with_context(|| {
        format!(
            "installing shell color profile defaults under '{}'",
            rootfs_source_dir.display()
        )
    })?;

    if install_experience == InstallExperience::Ux {
        let docs_cmd_path = rootfs_source_dir.join(INSTALL_DOCS);
        let docs_tui_cmd_path = rootfs_source_dir.join("usr/local/bin/docs-tui");
        match ux_docs_frontend {
            InstallDocsFrontend::BunBundle => {
                install_bun_docs_tui_payload(repo_root, rootfs_source_dir, &docs_cmd_path)
                    .with_context(|| {
                        format!(
                            "installing bun-based install docs payload for '{}'",
                            distro_id
                        )
                    })?
            }
            InstallDocsFrontend::PlainText { guide } => {
                let guide_source = repo_root.join(guide);
                let docs_text_path = rootfs_source_dir.join(INSTALL_DOCS_TEXT);
                let guide_text = fs::read_to_string(&guide_source).with_context(|| {
                    format!(
                        "reading the variant install guide '{}' (ux_docs_guide)",
                        guide_source.display()
                    )
                })?;
                write_text(&docs_text_path, &guide_text).with_context(|| {
                    format!(
                        "writing install guide payload '{}'",
                        docs_text_path.display()
                    )
                })?;
                let docs_cmd = format!(
                    "#!/bin/sh\n\
set -eu\n\
\n\
DOCS_FILE=\"/{docs_text}\"\n\
case \"${{TERM:-}}\" in\n\
    \"\"|dumb|vt100|vt102|linux)\n\
        export TERM=xterm-256color\n\
        ;;\n\
esac\n\
export COLORTERM=\"${{COLORTERM:-truecolor}}\"\n\
export FORCE_COLOR=\"${{FORCE_COLOR:-3}}\"\n\
if [ -n \"${{NO_COLOR:-}}\" ]; then\n\
    unset NO_COLOR\n\
fi\n\
\n\
if [ -f \"$DOCS_FILE\" ]; then\n\
    if [ \"${{LEVITATE_DOCS_PLAIN:-0}}\" != \"1\" ] && command -v less >/dev/null 2>&1; then\n\
        less \"$DOCS_FILE\" || true\n\
        echo \"Install guide closed. Run 'levitate-install-docs' to reopen it.\"\n\
    else\n\
        cat \"$DOCS_FILE\"\n\
    fi\n\
else\n\
    echo \"install docs payload missing at $DOCS_FILE\"\n\
fi\n\
\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
                    shell = interactive_shell,
                    docs_text = INSTALL_DOCS_TEXT
                );
                write_executable(&docs_cmd_path, &docs_cmd).with_context(|| {
                    format!(
                        "installing install docs command '{}'",
                        docs_cmd_path.display()
                    )
                })?;
            }
        }

        // Canonical docs-tui command alias for split-pane right-side launchers.
        let docs_tui_cmd = format!("#!/bin/sh\nexec /{INSTALL_DOCS} \"$@\"\n");
        write_executable(&docs_tui_cmd_path, &docs_tui_cmd).with_context(|| {
            format!(
                "installing install docs-tui alias '{}'",
                docs_tui_cmd_path.display()
            )
        })?;
    }

    let entrypoint_path = rootfs_source_dir.join(INSTALL_ENTRYPOINT);
    let entrypoint_script = match install_experience {
        InstallExperience::Ux => format!(
            "#!/bin/sh\n\
set -eu\n\
\n\
choose_install_helper() {{\n\
    if [ -x /usr/local/bin/{split} ]; then\n\
        printf '%s\\n' \"/usr/local/bin/{split}\"\n\
        return 0\n\
    fi\n\
    if command -v {split} >/dev/null 2>&1; then\n\
        printf '%s\\n' \"{split}\"\n\
        return 0\n\
    fi\n\
    if command -v levitate-install-docs >/dev/null 2>&1; then\n\
        printf '%s\\n' \"levitate-install-docs\"\n\
        return 0\n\
    fi\n\
    return 1\n\
}}\n\
\n\
if [ \"${{1:-}}\" = \"--probe\" ]; then\n\
    helper=\"$(choose_install_helper || true)\"\n\
    if [ -n \"$helper\" ]; then\n\
        printf '{probe}%s\\n' \"$helper\"\n\
        exit 0\n\
    fi\n\
    printf '{probe}none\\n'\n\
    exit 3\n\
fi\n\
\n\
echo \"[{distro}] install experience: UX\"\n\
echo \"Starting local interactive install helper if available...\"\n\
\n\
helper=\"$(choose_install_helper || true)\"\n\
if [ -n \"$helper\" ]; then\n\
    case \"$helper\" in\n\
        */{split}|{split})\n\
            if [ \"${{{smoke}:-0}}\" = \"1\" ]; then\n\
                exec \"$helper\" --smoke\n\
            fi\n\
            ;;\n\
    esac\n\
    exec \"$helper\"\n\
fi\n\
\n\
echo \"No local docs TUI binary found; dropping to shell.\"\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
            shell = interactive_shell,
            distro = distro_id,
            split = SPLIT_HELPER,
            probe = PROBE_PREFIX,
            smoke = SMOKE_ENV
        ),
        InstallExperience::AutomatedSsh => format!(
            "#!/bin/sh\n\
set -eu\n\
\n\
echo \"[{distro}] install experience: automated SSH\"\n\
echo \"This ISO profile is intended for SSH-driven automation (qcow2/.img pipelines).\"\n\
exec \"${{SHELL:-{shell}}}\" -l\n",
            shell = interactive_shell,
            distro = distro_id
        ),
    };
    write_executable(&entrypoint_path, &entrypoint_script).with_context(|| {
        format!(
            "installing install entrypoint script '{}'",
            entrypoint_path.display()
        )
    })?;
    if install_experience == InstallExperience::Ux {
        let ux_profile_path = rootfs_source_dir.join(INSTALL_UX_PROFILE);
        let ux_profile = "#!/bin/sh\n\
case \"$-\" in\n\
    *i*) ;;\n\
    *) return 0 ;;\n\
esac\n\
\n\
[ -n \"${TMUX:-}\" ] && return 0\n\
[ \"${LEVITATE_INSTALL_UX_LAUNCHED:-0}\" = \"1\" ] && return 0\n\
\n\
@BOOT_INJECTION_IMPORT@\n\
\n\
TTY=\"$(tty 2>/dev/null || true)\"\n\
if [ -z \"${LEVITATE_INSTALL_LEFT_CMD:-}\" ]; then\n\
    if [ -x /bin/bash ]; then\n\
        LEVITATE_INSTALL_LEFT_CMD=\"/bin/bash -il\"\n\
    else\n\
        LEVITATE_INSTALL_LEFT_CMD=\"/bin/sh -il\"\n\
    fi\n\
    export LEVITATE_INSTALL_LEFT_CMD\n\
fi\n\
if [ \"$TTY\" = \"/dev/tty1\" ]; then\n\
    :\n\
elif [ \"$TTY\" = \"/dev/ttyS0\" ] && [ \"${LEVITATE_INSTALL_SERIAL_UX:-0}\" = \"1\" ]; then\n\
    if [ -z \"${LEVITATE_INSTALL_RIGHT_CMD:-}\" ]; then\n\
        LEVITATE_INSTALL_RIGHT_CMD=\"docs-tui --slug installation\"\n\
        export LEVITATE_INSTALL_RIGHT_CMD\n\
    fi\n\
    export LEVITATE_DOCS_PLAIN=1\n\
    :\n\
else\n\
    return 0\n\
fi\n\
\n\
echo \"[install-ux] Launching install UX on $TTY...\"\n\
export LEVITATE_INSTALL_UX_LAUNCHED=1\n\
exec @INSTALL_ENTRYPOINT@\n";
        let ux_profile = crate::artifact::live_overlay::fill_guest_script(ux_profile);
        write_text(&ux_profile_path, &ux_profile).with_context(|| {
            format!(
                "installing install UX profile hook '{}'",
                ux_profile_path.display()
            )
        })?;
    }

    Ok(())
}

fn install_shell_color_profile(rootfs_source_dir: &Path) -> Result<()> {
    let profile_path = rootfs_source_dir.join("etc/profile.d/25-shell-color.sh");
    let profile = "#!/bin/sh\n\
case \"$-\" in\n\
    *i*) ;;\n\
    *) return 0 ;;\n\
esac\n\
\n\
[ \"${NO_COLOR:-0}\" = \"1\" ] && return 0\n\
\n\
if [ -z \"${TERM:-}\" ] || [ \"${TERM:-}\" = \"dumb\" ] || [ \"${TERM:-}\" = \"vt100\" ] || [ \"${TERM:-}\" = \"vt102\" ] || [ \"${TERM:-}\" = \"linux\" ]; then\n\
    TTY=\"$(tty 2>/dev/null || true)\"\n\
    case \"$TTY\" in\n\
        /dev/ttyS*|/dev/tty[0-9]*)\n\
            export TERM=xterm-256color\n\
            ;;\n\
    esac\n\
fi\n\
\n\
export CLICOLOR=\"${CLICOLOR:-1}\"\n\
export COLORTERM=\"${COLORTERM:-truecolor}\"\n\
export LESS=\"${LESS:--FRSX}\"\n\
export PAGER=\"${PAGER:-less}\"\n\
\n\
if command -v dircolors >/dev/null 2>&1; then\n\
    eval \"$(dircolors -b 2>/dev/null || true)\"\n\
fi\n\
\n\
if command -v ls >/dev/null 2>&1; then\n\
    alias ls='ls --color=auto'\n\
    alias ll='ls -alF --color=auto'\n\
    alias la='ls -A --color=auto'\n\
fi\n\
\n\
if command -v grep >/dev/null 2>&1; then\n\
    alias grep='grep --color=auto'\n\
    alias egrep='grep -E --color=auto'\n\
    alias fgrep='grep -F --color=auto'\n\
fi\n\
\n\
if [ -n \"${BASH_VERSION:-}\" ]; then\n\
    export HISTCONTROL=\"${HISTCONTROL:-ignoredups:erasedups}\"\n\
    export HISTSIZE=\"${HISTSIZE:-10000}\"\n\
    export HISTFILESIZE=\"${HISTFILESIZE:-20000}\"\n\
    export HISTTIMEFORMAT=\"${HISTTIMEFORMAT:-%F %T }\"\n\
    shopt -s histappend 2>/dev/null || true\n\
    shopt -s checkwinsize 2>/dev/null || true\n\
    if [ -r /usr/share/bash-completion/bash_completion ]; then\n\
        . /usr/share/bash-completion/bash_completion\n\
    elif [ -r /etc/bash_completion ]; then\n\
        . /etc/bash_completion\n\
    fi\n\
    export PS1=\"\\[\\033[1;32m\\]\\u@\\h\\[\\033[0m\\]:\\[\\033[1;34m\\]\\w\\[\\033[0m\\]\\\\$ \"\n\
fi\n";
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
