//! `ready-set --help` / `ready-set help`.

use std::ffi::OsString;

use ready_set_sdk::ExitCode;

use crate::env::EnvContract;

const HELP_TEXT: &str = include_str!("help.txt");

/// Print the meta help.
pub fn run(_args: &[OsString], _contract: &EnvContract) -> ExitCode {
    println!("{HELP_TEXT}");
    ExitCode::Ok
}
