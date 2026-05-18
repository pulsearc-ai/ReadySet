//! `ready-set --version`.

use std::ffi::OsString;

use ready_set_sdk::ExitCode;

use crate::env::EnvContract;

/// Print the dispatcher version.
pub fn run(_args: &[OsString], _contract: &EnvContract) -> ExitCode {
    println!("ready-set {}", env!("CARGO_PKG_VERSION"));
    ExitCode::Ok
}
