//! Argument parsing for auth provider lifecycle requests.

use std::ffi::OsString;

use clap::Parser;

/// Shared setup options for `__set`.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set-auth __set", about, long_about = None, no_binary_name = true)]
pub struct SetOptions {
    /// Replace generated plugin files even when their content has diverged.
    #[arg(long)]
    pub force: bool,

    /// Plan and report writes without modifying any files.
    #[arg(long)]
    pub dry_run: bool,
}

impl SetOptions {
    /// Parse from provider passthrough args.
    ///
    /// # Errors
    ///
    /// Returns a clap error formatted for direct printing.
    pub fn parse_args(args: &[OsString]) -> Result<Self, clap::Error> {
        Self::try_parse_from(args)
    }
}
