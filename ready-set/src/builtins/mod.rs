//! Built-in subcommands of the `ready-set` dispatcher.

pub mod go;
pub mod help;
pub mod list;
pub mod ready;
pub mod set;
pub mod version;

use ready_set_sdk::ExitCode;

use crate::env::EnvContract;

/// Type of a built-in handler. Returns the requested process exit code.
pub type BuiltinFn = fn(&[std::ffi::OsString], &EnvContract) -> ExitCode;

/// Look up a built-in by subcommand name.
#[must_use]
pub fn route(name: &str) -> Option<BuiltinFn> {
    match name {
        "go" => Some(go::run),
        "help" => Some(help::run),
        "version" => Some(version::run),
        "list" => Some(list::run),
        "ready" => Some(ready::run),
        "set" => Some(set::run),
        _ => None,
    }
}
