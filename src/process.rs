//! The one way the builder runs host programs, and the host programs it needs.
//!
//! Every child started through [`Cmd`] gets the builder's environment plus what the
//! call sets or removes, dies with the builder ([`die_with_parent`]), and is reported by
//! how it ended: its exit code, or the signal that killed it. A program that cannot be
//! started is reported as not found, not executable or not a valid executable.
//!
//! [`HOST_TOOLS`] is the one list of host programs the build, the base lock and the
//! scenarios need, with the package that provides each on dnf and apt hosts. A release
//! build checks it before any work ([`check_host_tools`]); `cargo xtask doctor` and CI
//! read the same list.

use anyhow::{anyhow, bail, Result};
use std::ffi::OsStr;
use std::io;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Makes the calling process receive SIGTERM when its parent exits, so no step
/// outlives the build that started it (and keeps holding its locks).
pub fn die_with_parent() -> io::Result<()> {
    // SAFETY: PR_SET_PDEATHSIG only sets an attribute of the calling process.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// How a child ended: `exit code N`, or `killed by SIGNAME (signal N)`.
pub fn exit_description(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit code {code}");
    }
    let Some(signal) = status.signal() else {
        return status.to_string();
    };
    let name = match signal {
        libc::SIGHUP => "SIGHUP",
        libc::SIGINT => "SIGINT",
        libc::SIGQUIT => "SIGQUIT",
        libc::SIGABRT => "SIGABRT",
        libc::SIGKILL => "SIGKILL",
        libc::SIGSEGV => "SIGSEGV",
        libc::SIGPIPE => "SIGPIPE",
        libc::SIGALRM => "SIGALRM",
        libc::SIGTERM => "SIGTERM",
        _ => "a signal",
    };
    format!("killed by {name} (signal {signal})")
}

/// The full path of `program` on PATH.
pub fn which(program: &str) -> Option<PathBuf> {
    which::which(program).ok()
}

/// A child's captured output.
#[derive(Debug)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

/// A host program to run.
pub struct Cmd {
    command: Command,
    program: String,
    dir: Option<PathBuf>,
    error_prefix: Option<String>,
    stdout_to_stderr: bool,
}

impl Cmd {
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        let mut command = Command::new(program.as_ref());
        // SAFETY: the hook only calls prctl, which is async-signal-safe.
        unsafe { command.pre_exec(die_with_parent) };
        Self {
            command,
            program: program.as_ref().to_string_lossy().into_owned(),
            dir: None,
            error_prefix: None,
            stdout_to_stderr: false,
        }
    }

    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.command.arg(arg);
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.command.args(args);
        self
    }

    pub fn dir(mut self, dir: &Path) -> Self {
        self.command.current_dir(dir);
        self.dir = Some(dir.to_path_buf());
        self
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.command.env(key, value);
        self
    }

    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.command.env_remove(key);
        self
    }

    /// What failed, for the error when the child fails (default: `'<program>' failed`).
    pub fn error_msg(mut self, msg: impl Into<String>) -> Self {
        self.error_prefix = Some(msg.into());
        self
    }

    /// Send the child's stdout to the builder's stderr, for children whose output is
    /// progress, so it never mixes with what a command prints as its result.
    pub fn stdout_to_stderr(mut self) -> Self {
        self.stdout_to_stderr = true;
        self
    }

    /// Run with output captured; fails with the child's stderr unless it succeeds.
    pub fn run(mut self) -> Result<Output> {
        self.check_dir()?;
        let output = self
            .command
            .stdin(Stdio::null())
            .output()
            .map_err(|error| self.spawn_error(error))?;
        let output_text = Output {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        if !output.status.success() {
            let stderr = output_text.stderr.trim();
            let failed = self.failed(output.status);
            if stderr.is_empty() {
                bail!("{failed}");
            }
            bail!("{failed}:\n{stderr}");
        }
        Ok(output_text)
    }

    /// Run with the terminal's stdin and stderr (and stdout, unless sent to stderr).
    pub fn run_interactive(mut self) -> Result<()> {
        self.check_dir()?;
        if self.stdout_to_stderr {
            self.command.stdout(Stdio::from(io::stderr()));
        }
        let status = self
            .command
            .status()
            .map_err(|error| self.spawn_error(error))?;
        if !status.success() {
            bail!("{}", self.failed(status));
        }
        Ok(())
    }

    fn check_dir(&self) -> Result<()> {
        match &self.dir {
            Some(dir) if !dir.is_dir() => bail!(
                "cannot run '{}': its working directory '{}' does not exist",
                self.program,
                dir.display()
            ),
            _ => Ok(()),
        }
    }

    fn failed(&self, status: ExitStatus) -> String {
        let prefix = self
            .error_prefix
            .clone()
            .unwrap_or_else(|| format!("'{}' failed", self.program));
        format!("{prefix} ({})", exit_description(status))
    }

    fn spawn_error(&self, error: io::Error) -> anyhow::Error {
        let program = &self.program;
        let why = match error.kind() {
            io::ErrorKind::NotFound if program.contains('/') => "does not exist".to_string(),
            io::ErrorKind::NotFound => "is not on PATH".to_string(),
            io::ErrorKind::PermissionDenied => "is not executable (permission denied)".to_string(),
            _ if error.raw_os_error() == Some(libc::ENOEXEC) => {
                "is not a valid executable (exec format error)".to_string()
            }
            _ => format!("could not be started: {error}"),
        };
        match &self.error_prefix {
            Some(prefix) => anyhow!("{prefix}: '{program}' {why}"),
            None => anyhow!("'{program}' {why}"),
        }
    }
}

// =============================================================================
// Host prerequisites
// =============================================================================

/// What a host program is needed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNeed {
    /// Building products and release ISOs.
    Build,
    /// `cargo xtask base lock` (resolving and verifying the Fedora base packages).
    BaseLock,
    /// Booting and logging in to scenario VMs.
    Scenarios,
}

/// A host's package manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostFamily {
    Dnf,
    Apt,
}

impl HostFamily {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dnf" => Some(Self::Dnf),
            "apt" => Some(Self::Apt),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Dnf => "dnf",
            Self::Apt => "apt",
        }
    }

    /// This host's family, from `ID` and `ID_LIKE` in `/etc/os-release`.
    pub fn detect() -> Option<Self> {
        let text = std::fs::read_to_string("/etc/os-release").ok()?;
        let ids: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("ID=").or(line.strip_prefix("ID_LIKE=")))
            .flat_map(|value| value.trim_matches('"').split_whitespace())
            .collect();
        if ids.iter().any(|id| ["fedora", "rhel", "centos"].contains(id)) {
            Some(Self::Dnf)
        } else if ids.iter().any(|id| ["debian", "ubuntu"].contains(id)) {
            Some(Self::Apt)
        } else {
            None
        }
    }
}

/// A host program, what needs it, and the packages that provide it, or `upstream`
/// for one not installed from the host's packages.
#[derive(Debug)]
pub struct HostTool {
    pub program: &'static str,
    pub purpose: &'static str,
    pub needs: &'static [HostNeed],
    pub dnf: &'static [&'static str],
    pub apt: &'static [&'static str],
    pub upstream: Option<&'static str>,
}

impl HostTool {
    pub fn packages(&self, family: HostFamily) -> &'static [&'static str] {
        match family {
            HostFamily::Dnf => self.dnf,
            HostFamily::Apt => self.apt,
        }
    }

    /// The command that installs it on a `family` host (both families' packages
    /// when the family is unknown).
    pub fn fix(&self, family: Option<HostFamily>) -> String {
        if let Some(upstream) = self.upstream {
            return format!("install {} from {upstream}", self.program);
        }
        match family {
            Some(family) => {
                format!("sudo {} install {}", family.name(), self.packages(family).join(" "))
            }
            None => format!(
                "install {} (dnf: {}; apt: {})",
                self.program,
                self.dnf.join(" "),
                self.apt.join(" ")
            ),
        }
    }

    /// Where it is on PATH, or why it cannot be used.
    pub fn status(&self) -> Result<PathBuf, String> {
        let path = which(self.program).ok_or_else(|| format!("not on PATH ({})", self.purpose))?;
        if self.program == "mkfs.erofs" {
            erofs_features(&path)?;
        }
        Ok(path)
    }
}

use HostNeed::{BaseLock, Build, Scenarios};

/// Every host program the build, the base lock and the scenarios run.
pub const HOST_TOOLS: &[HostTool] = &[
    HostTool {
        program: "cargo",
        purpose: "builds Recipe, the install tools, recinit and reciso",
        needs: &[Build],
        dnf: &[],
        apt: &[],
        upstream: Some("rustup (https://rustup.rs)"),
    },
    HostTool {
        program: "git",
        purpose: "lists a tool crate's files for its offline source",
        needs: &[Build],
        dnf: &["git"],
        apt: &["git"],
        upstream: None,
    },
    HostTool {
        program: "curl",
        purpose: "downloads busybox and the Fedora repository indexes",
        needs: &[Build, BaseLock],
        dnf: &["curl"],
        apt: &["curl"],
        upstream: None,
    },
    HostTool {
        program: "mkfs.erofs",
        purpose: "writes the rootfs and overlay images (erofs-utils 1.8+, with zstd and --tar)",
        needs: &[Build],
        dnf: &["erofs-utils"],
        apt: &["erofs-utils"],
        upstream: None,
    },
    HostTool {
        program: "fsck.erofs",
        purpose: "extracts a parent product's rootfs image",
        needs: &[Build],
        dnf: &["erofs-utils"],
        apt: &["erofs-utils"],
        upstream: None,
    },
    HostTool {
        program: "readelf",
        purpose: "recinit finds the libraries of initramfs binaries",
        needs: &[Build],
        dnf: &["binutils"],
        apt: &["binutils"],
        upstream: None,
    },
    HostTool {
        program: "xz",
        purpose: "decompresses kernel modules and repository indexes",
        needs: &[Build, BaseLock],
        dnf: &["xz"],
        apt: &["xz-utils"],
        upstream: None,
    },
    HostTool {
        program: "gzip",
        purpose: "decompresses kernel modules and repository indexes",
        needs: &[Build, BaseLock],
        dnf: &["gzip"],
        apt: &["gzip"],
        upstream: None,
    },
    HostTool {
        program: "zstd",
        purpose: "decompresses kernel modules and repository indexes",
        needs: &[Build, BaseLock],
        dnf: &["zstd"],
        apt: &["zstd"],
        upstream: None,
    },
    HostTool {
        program: "ukify",
        purpose: "builds the UKIs",
        needs: &[Build],
        dnf: &["systemd-ukify", "systemd-boot-unsigned"],
        apt: &["systemd-ukify", "systemd-boot-efi"],
        upstream: None,
    },
    HostTool {
        program: "xorriso",
        purpose: "writes the ISOs and the scenarios' boot injection media",
        needs: &[Build, Scenarios],
        dnf: &["xorriso"],
        apt: &["xorriso"],
        upstream: None,
    },
    HostTool {
        program: "mkfs.fat",
        purpose: "makes the ISO's EFI partition image",
        needs: &[Build],
        dnf: &["dosfstools"],
        apt: &["dosfstools"],
        upstream: None,
    },
    HostTool {
        program: "mmd",
        purpose: "fills the ISO's EFI partition image",
        needs: &[Build],
        dnf: &["mtools"],
        apt: &["mtools"],
        upstream: None,
    },
    HostTool {
        program: "mcopy",
        purpose: "fills the ISO's EFI partition image",
        needs: &[Build],
        dnf: &["mtools"],
        apt: &["mtools"],
        upstream: None,
    },
    HostTool {
        program: "sha512sum",
        purpose: "checksums the ISO",
        needs: &[Build],
        dnf: &["coreutils"],
        apt: &["coreutils"],
        upstream: None,
    },
    HostTool {
        program: "rpmkeys",
        purpose: "verifies the Fedora base packages' signatures",
        needs: &[BaseLock],
        dnf: &["rpm"],
        apt: &["rpm"],
        upstream: None,
    },
    HostTool {
        program: "virsh",
        purpose: "scenario VMs run under libvirt",
        needs: &[Scenarios],
        dnf: &["libvirt-client", "libvirt-daemon-kvm"],
        apt: &["libvirt-clients", "libvirt-daemon", "qemu-system-x86", "ovmf"],
        upstream: None,
    },
    HostTool {
        program: "passt",
        purpose: "user networking and the SSH forward of scenario VMs",
        needs: &[Scenarios],
        dnf: &["passt"],
        apt: &["passt"],
        upstream: None,
    },
    HostTool {
        program: "qemu-img",
        purpose: "scenario disk images",
        needs: &[Scenarios],
        dnf: &["qemu-img"],
        apt: &["qemu-utils"],
        upstream: None,
    },
    HostTool {
        program: "ssh",
        purpose: "logs in to scenario guests",
        needs: &[Scenarios],
        dnf: &["openssh-clients"],
        apt: &["openssh-client"],
        upstream: None,
    },
    HostTool {
        program: "ssh-keygen",
        purpose: "creates the repo SSH key",
        needs: &[Scenarios],
        dnf: &["openssh-clients"],
        apt: &["openssh-client"],
        upstream: None,
    },
];

/// Fail, naming every one, if a host program `need` requires is missing or unusable.
pub fn check_host_tools(need: HostNeed) -> Result<()> {
    let family = HostFamily::detect();
    let problems: Vec<String> = HOST_TOOLS
        .iter()
        .filter(|tool| tool.needs.contains(&need))
        .filter_map(|tool| {
            let why = tool.status().err()?;
            Some(format!("  {}: {why}\n    fix: {}", tool.program, tool.fix(family)))
        })
        .collect();
    if !problems.is_empty() {
        bail!(
            "this host lacks what {} needs:\n{}",
            match need {
                Build => "a build",
                BaseLock => "the base lock",
                Scenarios => "the scenarios",
            },
            problems.join("\n")
        );
    }
    Ok(())
}

/// The packages that provide every host program, for a `family` host.
pub fn host_packages(family: HostFamily) -> Vec<&'static str> {
    let mut packages: Vec<&str> = HOST_TOOLS
        .iter()
        .flat_map(|tool| tool.packages(family).iter().copied())
        .collect();
    packages.sort_unstable();
    packages.dedup();
    packages
}

/// mkfs.erofs must compress with zstd and import a tar stream (`--tar=f`): erofs-utils
/// 1.8 or newer. An older one would fail only after the multi-GB tar is written.
fn erofs_features(mkfs: &Path) -> Result<(), String> {
    let help = Cmd::new(mkfs)
        .arg("--help")
        .run()
        .map_err(|error| format!("{error:#}"))?
        .stdout;
    let missing: Vec<&str> = [("--tar=", "--tar"), ("zstd", "zstd")]
        .into_iter()
        .filter(|(text, _)| !help.contains(text))
        .map(|(_, feature)| feature)
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} lacks {}: erofs-utils 1.8 or newer is required",
        mkfs.display(),
        missing.join(" and ")
    ))
}
