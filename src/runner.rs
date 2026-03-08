//! Test execution engine.
//!
//! Runs test commands via [`std::process::Command`], captures output,
//! and delegates parsing to the appropriate adapter.

use std::process::Command;
use std::time::Instant;

use crate::adapters::{TestAdapter, TestCommand};
use crate::results::TestSuite;

/// Output from a test run, before adapter-specific parsing.
#[derive(Debug, Clone)]
pub struct RawOutput {
    /// Combined stdout.
    pub stdout: String,
    /// Combined stderr.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: Option<i32>,
    /// Wall-clock duration of the run.
    pub duration: std::time::Duration,
}

/// Execute a test command and return the raw output.
pub fn execute(cmd: &TestCommand) -> std::io::Result<RawOutput> {
    let start = Instant::now();

    let mut command = Command::new(&cmd.program);
    command.args(&cmd.args);

    if let Some(cwd) = &cmd.cwd {
        command.current_dir(cwd);
    }

    for (key, value) in &cmd.env {
        command.env(key, value);
    }

    let output = command.output()?;
    let duration = start.elapsed();

    Ok(RawOutput {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        exit_code: output.status.code(),
        duration,
    })
}

/// Execute a test command and parse the results using the given adapter.
pub fn run_and_parse(
    adapter: &dyn TestAdapter,
    cmd: &TestCommand,
) -> std::io::Result<TestSuite> {
    let raw = execute(cmd)?;

    // Combine stdout and stderr for parsing — some frameworks
    // print test results to stderr.
    let combined = format!("{}\n{}", raw.stdout, raw.stderr);
    let mut suite = adapter.parse_output(&combined);

    // Use wall-clock time if the adapter didn't extract a duration.
    if suite.duration.is_none() {
        suite.duration = Some(raw.duration);
    }

    Ok(suite)
}

/// Format a [`TestCommand`] as a shell-printable string (for debugging/display).
#[must_use]
pub fn format_command(cmd: &TestCommand) -> String {
    let mut parts = vec![cmd.program.clone()];
    parts.extend(cmd.args.iter().cloned());
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::TestCommand;

    #[test]
    fn format_simple_command() {
        let cmd = TestCommand {
            program: "cargo".into(),
            args: vec!["test".into(), "--".into(), "foo".into()],
            cwd: None,
            env: vec![],
        };
        assert_eq!(format_command(&cmd), "cargo test -- foo");
    }

    #[test]
    fn format_command_no_args() {
        let cmd = TestCommand {
            program: "pytest".into(),
            args: vec![],
            cwd: None,
            env: vec![],
        };
        assert_eq!(format_command(&cmd), "pytest");
    }

    #[test]
    fn execute_echo() {
        let cmd = TestCommand {
            program: "echo".into(),
            args: vec!["hello".into()],
            cwd: None,
            env: vec![],
        };
        let raw = execute(&cmd).unwrap();
        assert_eq!(raw.stdout.trim(), "hello");
        assert_eq!(raw.exit_code, Some(0));
        assert!(raw.duration.as_millis() < 5000);
    }

    #[test]
    fn execute_with_env() {
        let cmd = TestCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "echo $SHIREN_TEST_VAR".into()],
            cwd: None,
            env: vec![("SHIREN_TEST_VAR".into(), "42".into())],
        };
        let raw = execute(&cmd).unwrap();
        assert_eq!(raw.stdout.trim(), "42");
    }

    #[test]
    fn execute_with_cwd() {
        let cmd = TestCommand {
            program: "pwd".into(),
            args: vec![],
            cwd: Some("/tmp".into()),
            env: vec![],
        };
        let raw = execute(&cmd).unwrap();
        // On macOS /tmp is a symlink to /private/tmp.
        let out = raw.stdout.trim();
        assert!(out == "/tmp" || out == "/private/tmp");
    }

    #[test]
    fn execute_failing_command() {
        let cmd = TestCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "exit 1".into()],
            cwd: None,
            env: vec![],
        };
        let raw = execute(&cmd).unwrap();
        assert_eq!(raw.exit_code, Some(1));
    }

    #[test]
    fn execute_captures_stderr() {
        let cmd = TestCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "echo err >&2".into()],
            cwd: None,
            env: vec![],
        };
        let raw = execute(&cmd).unwrap();
        assert_eq!(raw.stderr.trim(), "err");
    }
}
