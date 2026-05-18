//! `ready-set` dispatcher entry point.

use std::process::ExitCode;

fn main() -> ExitCode {
    ready_set::run(std::env::args_os()).into()
}
