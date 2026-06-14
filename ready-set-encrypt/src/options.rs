//! Argument parsing for secrets provider lifecycle requests.

use std::ffi::OsString;

use clap::Parser;

/// Shared setup options for `__set`.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set-encrypt __set", about, long_about = None, no_binary_name = true)]
pub struct SetOptions {
    /// Replace files even if their content has diverged from the template.
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

/// Options for rotation. Defaults to dry-run; `--confirm` is required for
/// actual mutations.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set-encrypt __go rotation", about, long_about = None, no_binary_name = true)]
pub struct RotateOptions {
    /// Actually execute rotation. Without this flag, rotation only
    /// prints what would happen and exits 0.
    #[arg(long)]
    pub confirm: bool,

    /// Rotate only the named secret. Can be passed more than once.
    #[arg(long = "name", value_name = "SECRET")]
    pub names: Vec<String>,
}

impl RotateOptions {
    /// Parse from provider passthrough args.
    ///
    /// Tolerates a leading `--` separator from the lifecycle passthrough
    /// protocol; `ready-set rotate --confirm` normally passes no separator.
    ///
    /// # Errors
    ///
    /// Returns a clap error formatted for direct printing.
    pub fn parse_args(args: &[OsString]) -> Result<Self, clap::Error> {
        let trimmed: Vec<OsString> = match args.first() {
            Some(first) if first == "--" => args[1..].to_vec(),
            _ => args.to_vec(),
        };
        Self::try_parse_from(trimmed)
    }
}
