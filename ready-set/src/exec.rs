//! Hand off execution to a discovered plugin binary.
//!
//! On Unix the dispatcher uses `execvp`-style replacement so the plugin
//! inherits the dispatcher's PID — cleaner signal handling, matches cargo.
//! On Windows there is no `execvp`; we spawn the plugin as a child via
//! `Command::status()` and propagate its exit code.

use std::ffi::OsString;
use std::process::Command;

use ready_set_sdk::ExitCode;

use crate::discovery::PluginEntry;
use crate::env::{EnvContract, export_contract};

/// Dispatch into `entry` with `args`, exporting the env contract.
///
/// Returns the plugin's exit code.
#[cfg(unix)]
pub fn dispatch_to_plugin(
    entry: &PluginEntry,
    args: &[OsString],
    contract: &EnvContract,
) -> ExitCode {
    use std::os::unix::process::CommandExt;

    let mut cmd = Command::new(&entry.binary_path);
    cmd.args(args);
    export_contract(&mut cmd, contract);
    let err = cmd.exec();
    // `exec` only returns on failure.
    eprintln!(
        "ready-set: failed to exec {}: {err}",
        entry.binary_path.display()
    );
    ExitCode::SystemError
}

/// Dispatch into `entry` with `args`, exporting the env contract.
///
/// Returns the plugin's exit code.
#[cfg(windows)]
pub fn dispatch_to_plugin(
    entry: &PluginEntry,
    args: &[OsString],
    contract: &EnvContract,
) -> ExitCode {
    let mut cmd = Command::new(&entry.binary_path);
    cmd.args(args);
    export_contract(&mut cmd, contract);
    match cmd.status() {
        Ok(status) => match status.code() {
            Some(0) => ExitCode::Ok,
            Some(127) => ExitCode::UnknownSubcommand,
            Some(_) | None => ExitCode::SystemError,
        },
        Err(err) => {
            eprintln!(
                "ready-set: failed to spawn {}: {err}",
                entry.binary_path.display()
            );
            ExitCode::SystemError
        },
    }
}
