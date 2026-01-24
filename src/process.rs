//! Centralized command execution with consistent error handling.
//!
//! This module provides a unified API for running external commands,
//! ensuring all commands capture stderr and provide useful error messages.

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    /// Exit status of the command.
    pub status: ExitStatus,
    /// Captured stdout as a string.
    pub stdout: String,
    /// Captured stderr as a string.
    pub stderr: String,
}

impl CommandResult {
    /// Returns true if the command exited successfully.
    pub fn success(&self) -> bool {
        self.status.success()
    }

    /// Get the exit code, or -1 if terminated by signal.
    pub fn code(&self) -> i32 {
        self.status.code().unwrap_or(-1)
    }

    /// Get a human-readable exit description.
    /// Returns "exit code N" for normal exits, or "killed by SIGNAL" for signal termination.
    pub fn exit_description(&self) -> String {
        if let Some(code) = self.status.code() {
            format!("exit code {}", code)
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = self.status.signal() {
                    let signal_name = match signal {
                        1 => "SIGHUP",
                        2 => "SIGINT",
                        3 => "SIGQUIT",
                        6 => "SIGABRT",
                        9 => "SIGKILL",
                        11 => "SIGSEGV",
                        13 => "SIGPIPE",
                        14 => "SIGALRM",
                        15 => "SIGTERM",
                        _ => "unknown signal",
                    };
                    return format!("killed by {} (signal {})", signal_name, signal);
                }
            }
            "terminated by signal".to_string()
        }
    }

    /// Get stdout, trimmed of whitespace.
    pub fn stdout_trimmed(&self) -> &str {
        self.stdout.trim()
    }

    /// Get stderr, trimmed of whitespace.
    pub fn stderr_trimmed(&self) -> &str {
        self.stderr.trim()
    }
}

/// Builder for configuring command execution.
pub struct Cmd {
    program: String,
    args: Vec<String>,
    current_dir: Option<std::path::PathBuf>,
    /// If true, don't fail on non-zero exit.
    allow_fail: bool,
    /// Custom error message prefix.
    error_prefix: Option<String>,
}

impl Cmd {
    /// Create a new command builder.
    pub fn new(program: impl AsRef<str>) -> Self {
        Self {
            program: program.as_ref().to_string(),
            args: Vec::new(),
            current_dir: None,
            allow_fail: false,
            error_prefix: None,
        }
    }

    /// Add a single argument.
    pub fn arg(mut self, arg: impl AsRef<str>) -> Self {
        self.args.push(arg.as_ref().to_string());
        self
    }

    /// Add multiple arguments.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for arg in args {
            self.args.push(arg.as_ref().to_string());
        }
        self
    }

    /// Add a path as an argument.
    pub fn arg_path(mut self, path: &Path) -> Self {
        self.args.push(path.to_string_lossy().into_owned());
        self
    }

    /// Set the working directory.
    pub fn dir(mut self, dir: &Path) -> Self {
        self.current_dir = Some(dir.to_path_buf());
        self
    }

    /// Allow non-zero exit codes without failing.
    pub fn allow_fail(mut self) -> Self {
        self.allow_fail = true;
        self
    }

    /// Set a custom error message prefix.
    pub fn error_msg(mut self, msg: impl AsRef<str>) -> Self {
        self.error_prefix = Some(msg.as_ref().to_string());
        self
    }

    /// Run the command and capture output.
    pub fn run(self) -> Result<CommandResult> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);

        if let Some(ref dir) = self.current_dir {
            cmd.current_dir(dir);
        }

        let output = cmd.output().with_context(|| {
            format!(
                "Failed to execute '{}'. Is it installed?",
                self.program
            )
        })?;

        let result = CommandResult {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };

        if !self.allow_fail && !result.success() {
            let prefix = self
                .error_prefix
                .unwrap_or_else(|| format!("'{}' failed", self.program));

            let stderr = result.stderr_trimmed();
            let exit_desc = result.exit_description();
            if stderr.is_empty() {
                bail!("{} ({})", prefix, exit_desc);
            } else {
                bail!("{} ({}):\n{}", prefix, exit_desc, stderr);
            }
        }

        Ok(result)
    }

    /// Run the command with inherited stdio (interactive/streaming).
    ///
    /// Output goes directly to the terminal. Use for long-running commands
    /// where the user should see progress (e.g., kernel builds).
    pub fn run_interactive(self) -> Result<ExitStatus> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args);
        cmd.stdin(Stdio::inherit());
        cmd.stdout(Stdio::inherit());
        cmd.stderr(Stdio::inherit());

        if let Some(ref dir) = self.current_dir {
            cmd.current_dir(dir);
        }

        let status = cmd.status().with_context(|| {
            format!(
                "Failed to execute '{}'. Is it installed?",
                self.program
            )
        })?;

        if !self.allow_fail && !status.success() {
            let prefix = self
                .error_prefix
                .unwrap_or_else(|| format!("'{}' failed", self.program));
            let exit_desc = exit_description_from_status(&status);
            bail!("{} ({})", prefix, exit_desc);
        }

        Ok(status)
    }
}

/// Get a human-readable exit description from an ExitStatus.
fn exit_description_from_status(status: &ExitStatus) -> String {
    if let Some(code) = status.code() {
        format!("exit code {}", code)
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(signal) = status.signal() {
                let signal_name = match signal {
                    1 => "SIGHUP",
                    2 => "SIGINT",
                    3 => "SIGQUIT",
                    6 => "SIGABRT",
                    9 => "SIGKILL",
                    11 => "SIGSEGV",
                    13 => "SIGPIPE",
                    14 => "SIGALRM",
                    15 => "SIGTERM",
                    _ => "unknown signal",
                };
                return format!("killed by {} (signal {})", signal_name, signal);
            }
        }
        "terminated by signal".to_string()
    }
}

// =============================================================================
// Convenience functions
// =============================================================================

/// Run a command with arguments. Fails with stderr on error.
///
/// # Example
/// ```ignore
/// let result = run("ls", &["-la", "/tmp"])?;
/// println!("Files:\n{}", result.stdout);
/// ```
pub fn run<I, S>(program: &str, args: I) -> Result<CommandResult>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut cmd = Cmd::new(program);
    for arg in args {
        cmd = cmd.arg(arg);
    }
    cmd.run()
}

/// Run a command in a specific directory.
pub fn run_in<I, S>(program: &str, args: I, dir: &Path) -> Result<CommandResult>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut cmd = Cmd::new(program).dir(dir);
    for arg in args {
        cmd = cmd.arg(arg);
    }
    cmd.run()
}

/// Run a shell command via `sh -c`.
///
/// # Example
/// ```ignore
/// let result = shell("echo hello && echo world")?;
/// ```
pub fn shell(command: &str) -> Result<CommandResult> {
    run("sh", ["-c", command])
}

/// Run a shell command in a specific directory.
pub fn shell_in(command: &str, dir: &Path) -> Result<CommandResult> {
    run_in("sh", ["-c", command], dir)
}

/// Check if a program exists in PATH.
///
/// Returns the full path if found, None otherwise.
pub fn which(program: &str) -> Option<String> {
    let result = Cmd::new("which").arg(program).allow_fail().run().ok()?;

    if result.success() {
        let path = result.stdout_trimmed();
        if !path.is_empty() {
            return Some(path.to_string());
        }
    }
    None
}

/// Check if a program exists in PATH (bool version).
pub fn exists(program: &str) -> bool {
    which(program).is_some()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_success() {
        let result = run("echo", ["hello"]).unwrap();
        assert!(result.success());
        assert_eq!(result.stdout_trimmed(), "hello");
    }

    #[test]
    fn test_run_captures_stderr() {
        // `ls` on a non-existent file writes to stderr
        let result = Cmd::new("ls")
            .arg("/nonexistent_path_12345")
            .allow_fail()
            .run()
            .unwrap();

        assert!(!result.success());
        assert!(!result.stderr.is_empty());
    }

    #[test]
    fn test_run_failure_includes_stderr() {
        let err = run("ls", ["/nonexistent_path_12345"]).unwrap_err();
        let msg = err.to_string();

        // Error message should include the stderr
        assert!(msg.contains("No such file") || msg.contains("cannot access"));
    }

    #[test]
    fn test_shell_command() {
        let result = shell("echo hello && echo world").unwrap();
        assert!(result.success());
        assert!(result.stdout.contains("hello"));
        assert!(result.stdout.contains("world"));
    }

    #[test]
    fn test_which_exists() {
        // `sh` should exist on any Unix system
        assert!(which("sh").is_some());
    }

    #[test]
    fn test_which_not_exists() {
        assert!(which("nonexistent_program_12345").is_none());
    }

    #[test]
    fn test_exists() {
        assert!(exists("sh"));
        assert!(!exists("nonexistent_program_12345"));
    }

    #[test]
    fn test_cmd_builder_chaining() {
        let result = Cmd::new("echo")
            .arg("hello")
            .arg("world")
            .run()
            .unwrap();

        assert_eq!(result.stdout_trimmed(), "hello world");
    }

    #[test]
    fn test_cmd_args_iterator() {
        let args = vec!["one", "two", "three"];
        let result = Cmd::new("echo").args(args).run().unwrap();

        assert_eq!(result.stdout_trimmed(), "one two three");
    }

    #[test]
    fn test_custom_error_message() {
        let err = Cmd::new("false") // `false` always exits with 1
            .error_msg("Custom build step failed")
            .run()
            .unwrap_err();

        assert!(err.to_string().contains("Custom build step failed"));
    }

    #[test]
    fn test_allow_fail() {
        let result = Cmd::new("false").allow_fail().run().unwrap();

        assert!(!result.success());
        assert_eq!(result.code(), 1);
    }

    #[test]
    fn test_run_in_directory() {
        let result = run_in("pwd", [] as [&str; 0], Path::new("/tmp")).unwrap();
        assert!(result.stdout_trimmed().contains("tmp"));
    }
}
