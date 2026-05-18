//! Argument parsing for Rust provider lifecycle requests.

use std::ffi::OsString;

use clap::Parser;

/// Shared setup options for `__set`.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set-rust __set", about, long_about = None, no_binary_name = true)]
#[allow(clippy::struct_excessive_bools)]
pub struct SetOptions {
    /// Replace files even if their content has diverged from the template.
    #[arg(long)]
    pub force: bool,

    /// Plan and report writes without modifying any files.
    #[arg(long)]
    pub dry_run: bool,

    /// Explicit member path to add to `[workspace.members]`. Repeatable.
    #[arg(long = "member")]
    pub members: Vec<String>,

    /// Skip recursive crate discovery.
    #[arg(long)]
    pub no_discover: bool,
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
