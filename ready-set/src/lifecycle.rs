//! Core lifecycle dispatch to provider plugins.

use std::ffi::OsString;
use std::process::{Command, ExitStatus, Stdio};

use ready_set_sdk::{CapabilityReport, CapabilityRunReport, ExitCode, ProviderId};

use crate::discovery::find_plugin;
use crate::env::{EnvContract, export_contract};

/// Result of invoking a provider `__ready` command.
#[derive(Debug)]
pub enum ReadyInvocation {
    /// Provider returned a valid report.
    Report(CapabilityReport),
    /// No provider binary was found.
    ProviderUnavailable {
        /// Human-readable summary.
        summary: String,
    },
    /// The provider ran but did not return a usable readiness report.
    ProviderFailed {
        /// Human-readable summary.
        summary: String,
    },
}

/// Result of invoking a provider `__set` command.
#[derive(Debug)]
pub enum SetInvocation {
    /// Provider completed and returned a report.
    Report(CapabilityRunReport),
    /// Human-mode provider process exited with a code.
    Streamed {
        /// Mapped dispatcher exit code.
        exit_code: ExitCode,
    },
    /// No provider binary was found.
    ProviderUnavailable {
        /// Human-readable summary.
        summary: String,
    },
    /// The provider ran but did not return a usable run report.
    ProviderFailed {
        /// Mapped dispatcher exit code.
        exit_code: ExitCode,
        /// Human-readable summary.
        summary: String,
    },
}

/// Result of invoking a provider `__go` command.
#[derive(Debug)]
pub enum GoInvocation {
    /// Provider completed and returned a report.
    Report {
        /// Structured run report.
        report: CapabilityRunReport,
        /// Mapped dispatcher exit code.
        exit_code: ExitCode,
    },
    /// Human-mode provider process exited with a code.
    Streamed {
        /// Mapped dispatcher exit code.
        exit_code: ExitCode,
    },
    /// No provider binary was found.
    ProviderUnavailable {
        /// Human-readable summary.
        summary: String,
    },
    /// The provider ran but did not return a usable run report.
    ProviderFailed {
        /// Mapped dispatcher exit code.
        exit_code: ExitCode,
        /// Human-readable summary.
        summary: String,
    },
}

/// Invoke `<provider> __ready <capability>` and parse its JSON report.
///
/// # Errors
///
/// Returns I/O errors from spawning the provider process.
pub fn invoke_ready(
    provider: &ProviderId,
    capability: &str,
    contract: &EnvContract,
) -> std::io::Result<ReadyInvocation> {
    let Some(entry) = find_plugin(provider.as_str()) else {
        return Ok(ReadyInvocation::ProviderUnavailable {
            summary: format!("provider `{provider}` is not installed"),
        });
    };

    let output = command_for_provider(&entry.binary_path, contract)
        .arg("__ready")
        .arg(capability)
        .env("READY_SET_OUTPUT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Ok(ReadyInvocation::ProviderFailed {
            summary: provider_failure_summary(provider, "__ready", &output.stderr),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<CapabilityReport>(stdout.trim()) {
        Ok(report) => Ok(ReadyInvocation::Report(report)),
        Err(err) => Ok(ReadyInvocation::ProviderFailed {
            summary: format!("provider `{provider}` returned invalid readiness JSON: {err}"),
        }),
    }
}

/// Invoke `<provider> __set <capability> ...`.
///
/// When `capture_json` is true, stdout is parsed as a
/// [`CapabilityRunReport`]. Otherwise stdout/stderr stream directly.
///
/// # Errors
///
/// Returns I/O errors from spawning the provider process.
pub fn invoke_set(
    provider: &ProviderId,
    capability: &str,
    args: &[OsString],
    contract: &EnvContract,
    capture_json: bool,
) -> std::io::Result<SetInvocation> {
    let Some(entry) = find_plugin(provider.as_str()) else {
        return Ok(SetInvocation::ProviderUnavailable {
            summary: format!("provider `{provider}` is not installed"),
        });
    };

    let mut cmd = command_for_provider(&entry.binary_path, contract);
    cmd.arg("__set").arg(capability).args(args);

    if !capture_json {
        let status = cmd.status()?;
        return Ok(SetInvocation::Streamed {
            exit_code: exit_code_from_status(status),
        });
    }

    let output = cmd
        .env("READY_SET_OUTPUT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let exit_code = exit_code_from_status(output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<CapabilityRunReport>(stdout.trim()) {
        Ok(report) if output.status.success() => Ok(SetInvocation::Report(report)),
        Ok(_) => Ok(SetInvocation::ProviderFailed {
            exit_code,
            summary: provider_failure_summary(provider, "__set", &output.stderr),
        }),
        Err(err) => Ok(SetInvocation::ProviderFailed {
            exit_code: ExitCode::ContractViolation,
            summary: format!("provider `{provider}` returned invalid set JSON: {err}"),
        }),
    }
}

/// Invoke `<provider> __go <capability> ...`.
///
/// When `capture_json` is true, stdout is parsed as a
/// [`CapabilityRunReport`]. Unlike `set`, a nonzero provider exit can still
/// return a valid failed workflow report.
///
/// # Errors
///
/// Returns I/O errors from spawning the provider process.
pub fn invoke_go(
    provider: &ProviderId,
    capability: &str,
    args: &[OsString],
    contract: &EnvContract,
    capture_json: bool,
) -> std::io::Result<GoInvocation> {
    let Some(entry) = find_plugin(provider.as_str()) else {
        return Ok(GoInvocation::ProviderUnavailable {
            summary: format!("provider `{provider}` is not installed"),
        });
    };

    let mut cmd = command_for_provider(&entry.binary_path, contract);
    cmd.arg("__go").arg(capability).args(args);

    if !capture_json {
        let status = cmd.status()?;
        return Ok(GoInvocation::Streamed {
            exit_code: exit_code_from_status(status),
        });
    }

    let output = cmd
        .env("READY_SET_OUTPUT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?;
    let exit_code = exit_code_from_status(output.status);
    let stdout = String::from_utf8_lossy(&output.stdout);
    match serde_json::from_str::<CapabilityRunReport>(stdout.trim()) {
        Ok(report) => Ok(GoInvocation::Report { report, exit_code }),
        Err(err) => Ok(GoInvocation::ProviderFailed {
            exit_code: ExitCode::ContractViolation,
            summary: format!("provider `{provider}` returned invalid go JSON: {err}"),
        }),
    }
}

fn command_for_provider(binary: &std::path::Path, contract: &EnvContract) -> Command {
    let mut cmd = Command::new(binary);
    export_contract(&mut cmd, contract);
    cmd
}

fn provider_failure_summary(provider: &ProviderId, command: &str, stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let detail = stderr.trim();
    if detail.is_empty() {
        format!("provider `{provider}` {command} failed")
    } else {
        format!("provider `{provider}` {command} failed: {detail}")
    }
}

// Each arm documents one row of `docs/contracts/exit-codes.md`; `Some(2)` is
// kept explicit even though the catch-all also returns `SystemError`.
#[allow(clippy::match_same_arms)]
fn exit_code_from_status(status: ExitStatus) -> ExitCode {
    if let Some(code) = status.code() {
        return match code {
            0 => ExitCode::Ok,
            1 => ExitCode::UserError,
            2 => ExitCode::SystemError,
            3 => ExitCode::DependencyMissing,
            4 => ExitCode::ProjectRequirementMissing,
            5 => ExitCode::ContractViolation,
            127 => ExitCode::UnknownSubcommand,
            _ => ExitCode::SystemError,
        };
    }
    signaled_or_system(status)
}

#[cfg(unix)]
fn signaled_or_system(status: ExitStatus) -> ExitCode {
    use std::os::unix::process::ExitStatusExt;
    status
        .signal()
        .and_then(|s| u8::try_from(s).ok())
        .map_or(ExitCode::SystemError, ExitCode::Signaled)
}

#[cfg(not(unix))]
const fn signaled_or_system(_status: ExitStatus) -> ExitCode {
    ExitCode::SystemError
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::*;

    // Construct a synthetic ExitStatus for a normal exit with `code`.
    // Linux/macOS waitpid status: bits 8-15 hold the exit code; bits 0-6 are 0.
    fn exited(code: i32) -> ExitStatus {
        ExitStatus::from_raw(code << 8)
    }

    // Construct a synthetic ExitStatus for a process killed by `signum`.
    // bits 0-6 hold the signal number; bits 8-15 are 0.
    fn signaled(signum: i32) -> ExitStatus {
        ExitStatus::from_raw(signum)
    }

    #[test]
    fn maps_documented_exit_codes() {
        assert_eq!(exit_code_from_status(exited(0)), ExitCode::Ok);
        assert_eq!(exit_code_from_status(exited(1)), ExitCode::UserError);
        assert_eq!(exit_code_from_status(exited(2)), ExitCode::SystemError);
        assert_eq!(
            exit_code_from_status(exited(3)),
            ExitCode::DependencyMissing
        );
        assert_eq!(
            exit_code_from_status(exited(4)),
            ExitCode::ProjectRequirementMissing
        );
        assert_eq!(
            exit_code_from_status(exited(5)),
            ExitCode::ContractViolation
        );
        assert_eq!(
            exit_code_from_status(exited(127)),
            ExitCode::UnknownSubcommand
        );
    }

    #[test]
    fn unrecognized_exit_code_falls_back_to_system_error() {
        assert_eq!(exit_code_from_status(exited(99)), ExitCode::SystemError);
        assert_eq!(exit_code_from_status(exited(42)), ExitCode::SystemError);
    }

    #[test]
    fn maps_signals_to_signaled_variant() {
        // SIGINT
        assert_eq!(exit_code_from_status(signaled(2)), ExitCode::Signaled(2));
        // SIGKILL
        assert_eq!(exit_code_from_status(signaled(9)), ExitCode::Signaled(9));
        // SIGTERM
        assert_eq!(exit_code_from_status(signaled(15)), ExitCode::Signaled(15));
    }

    #[test]
    fn signaled_emits_posix_shell_exit_code() {
        // POSIX convention: shell reports 128 + signum.
        assert_eq!(exit_code_from_status(signaled(2)).as_u8(), 130);
        assert_eq!(exit_code_from_status(signaled(15)).as_u8(), 143);
    }
}
