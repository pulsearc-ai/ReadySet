//! Sandboxed argv execution + template substitution for the `exec` backend.
//!
//! Substitution happens inside argv *elements only* — never through a shell.
//! This means `["bash", "-c", "fly secrets set X={{value}}"]` substitutes
//! `{{value}}` into the third argv element verbatim, but the plugin itself
//! never invokes `sh -c` on the user's behalf.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use ready_set_sdk::{Error, Result};

use crate::manifest::SecretEntry;
use crate::sandbox::{self, SandboxConfig};

/// Bound on captured stdout from a single command. Larger output is truncated
/// with a marker on stderr.
pub const STDOUT_CAPTURE_LIMIT: usize = 1024 * 1024;

/// What `run_command` returns. `stdout` is the silently-captured stream
/// (subject to `STDOUT_CAPTURE_LIMIT`). `stderr` is *not* captured — it
/// streams to the parent's stderr in real time.
#[derive(Debug)]
pub struct CommandOutput {
    /// Captured stdout (bounded).
    pub stdout: Vec<u8>,
    /// Process exit status.
    pub status: ExitStatus,
    /// Was the wrapped argv sandboxed via `sandbox-exec`?
    pub sandboxed: bool,
    /// Sandbox backend label, when sandboxed.
    pub platform_label: Option<&'static str>,
}

/// Substitute `{{value}}` and `{{value_path}}` inside argv elements.
///
/// Replacement is per-element string `replace`; no shell parsing.
#[must_use]
pub fn substitute_argv(argv: &[String], value: &str, value_path: &Path) -> Vec<String> {
    let value_path_str = value_path.display().to_string();
    argv.iter()
        .map(|a| {
            a.replace("{{value}}", value)
                .replace("{{value_path}}", &value_path_str)
        })
        .collect()
}

/// Run one command, sandboxed when applicable. Stdout is captured silently up
/// to `STDOUT_CAPTURE_LIMIT`; stderr passes through to the parent process.
///
/// # Errors
///
/// Returns [`Error::MissingDependency`] when the underlying tool (or
/// `sandbox-exec`) is not on PATH, and [`Error::Io`] for spawn/wait failures.
pub fn run_command(root: &Path, entry: &SecretEntry, argv: &[String]) -> Result<CommandOutput> {
    if argv.is_empty() {
        return Err(Error::contract("argv must not be empty"));
    }
    let config = sandbox_config_from_entry(root, entry);
    let wrap = sandbox::wrap(argv.to_vec(), &config)?;
    let wrapped = wrap.argv;

    which::which(&wrapped[0]).map_err(|_| Error::MissingDependency {
        name: wrapped[0].clone(),
        hint: Some(format!(
            "ensure `{}` is on PATH before running rotation",
            wrapped[0]
        )),
    })?;

    let mut child = Command::new(&wrapped[0])
        .args(&wrapped[1..])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(Error::Io)?;

    let mut stdout = Vec::new();
    if let Some(mut out) = child.stdout.take() {
        let mut buf = [0_u8; 8192];
        loop {
            match out.read(&mut buf).map_err(Error::Io)? {
                0 => break,
                n => {
                    if stdout.len() + n > STDOUT_CAPTURE_LIMIT {
                        let remaining = STDOUT_CAPTURE_LIMIT.saturating_sub(stdout.len());
                        stdout.extend_from_slice(&buf[..remaining]);
                        eprintln!(
                            "ready-set-encrypt: stdout truncated at {STDOUT_CAPTURE_LIMIT} bytes"
                        );
                        // Drain remaining bytes silently so the child can exit.
                        while out.read(&mut buf).map_err(Error::Io)? > 0 {}
                        break;
                    }
                    stdout.extend_from_slice(&buf[..n]);
                },
            }
        }
    }

    let status = child.wait().map_err(Error::Io)?;
    Ok(CommandOutput {
        stdout,
        status,
        sandboxed: wrap.sandboxed,
        platform_label: wrap.platform_label,
    })
}

/// Build a `SandboxConfig` from a manifest entry.
#[must_use]
pub fn sandbox_config_from_entry(root: &Path, entry: &SecretEntry) -> SandboxConfig {
    SandboxConfig {
        project_root: root.to_path_buf(),
        extra_write_paths: entry
            .sandbox_write_paths
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(PathBuf::from)
            .collect(),
        unsandboxed: entry.unsandboxed.unwrap_or(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_value_into_argv_element() {
        let argv = vec![
            "fly".to_owned(),
            "secrets".to_owned(),
            "set".to_owned(),
            "TOKEN={{value}}".to_owned(),
            "-a".to_owned(),
            "app".to_owned(),
        ];
        let out = substitute_argv(&argv, "deadbeef", Path::new("/tmp/v"));
        assert_eq!(out[3], "TOKEN=deadbeef");
        assert_eq!(out[0], "fly");
    }

    #[test]
    fn substitute_value_path_into_argv_element() {
        let argv = vec!["cat".to_owned(), "{{value_path}}".to_owned()];
        let out = substitute_argv(&argv, "ignored", Path::new("/tmp/v.txt"));
        assert_eq!(out, vec!["cat".to_owned(), "/tmp/v.txt".to_owned()]);
    }

    #[test]
    fn substitute_no_placeholder_passthrough() {
        let argv = vec!["echo".to_owned(), "hello".to_owned()];
        let out = substitute_argv(&argv, "v", Path::new("/p"));
        assert_eq!(out, argv);
    }

    #[test]
    fn run_command_rejects_empty_argv() {
        let dir = tempfile::tempdir().unwrap();
        let entry = SecretEntry::default();
        let err = run_command(dir.path(), &entry, &[]).unwrap_err();
        assert!(matches!(err, Error::ContractViolation(_)));
    }

    #[test]
    fn run_command_propagates_missing_tool() {
        let dir = tempfile::tempdir().unwrap();
        let entry = SecretEntry {
            unsandboxed: Some(true),
            ..SecretEntry::default()
        };
        let argv = vec!["this-binary-definitely-does-not-exist-xyz".to_owned()];
        let err = run_command(dir.path(), &entry, &argv).unwrap_err();
        assert!(matches!(err, Error::MissingDependency { .. }));
    }
}
