//! Argument parsing for the dispatcher's meta-command surface only.
//!
//! Plugin args are passed through verbatim; clap is **not** used to validate
//! plugin args. The dispatcher recognizes:
//!
//! * `--help`, `-h`
//! * `--version`, `-V`
//! * `--list` (with optional `--all`)
//! * Global flags that adjust the env contract: `--quiet`, `--verbose`,
//!   `--json`, `--color <auto|always|never>`. These are consumed by the
//!   dispatcher *only* if they appear before the subcommand name; once a
//!   subcommand is identified, all subsequent args go to the plugin.
//!
//! This is similar to how `cargo` and `git` parse their meta flags.

use std::ffi::{OsStr, OsString};

use ready_set_sdk::OutputMode;
use ready_set_sdk::context::{ColorMode, LogLevel};

/// Result of parsing the dispatcher's meta command line.
#[derive(Debug)]
pub enum ParsedArgs {
    /// Show meta help.
    Help,
    /// Show dispatcher version.
    Version,
    /// List built-ins and discovered plugins.
    List {
        /// Whether to include plugins whose `platforms` exclude the current OS.
        all: bool,
        /// Globals collected before/around `--list`.
        globals: GlobalFlags,
    },
    /// Resolve a subcommand (built-in or plugin) and pass through remaining args.
    Subcommand {
        /// Subcommand name without the `ready-set-` prefix.
        name: String,
        /// Args to forward to the subcommand.
        args: Vec<OsString>,
        /// Global flags that affect the env contract.
        globals: GlobalFlags,
    },
    /// No subcommand was provided.
    Empty {
        /// Globals collected before the empty boundary.
        globals: GlobalFlags,
    },
}

/// Global flags consumed by the dispatcher.
#[derive(Debug, Clone, Default)]
pub struct GlobalFlags {
    /// Output mode override.
    pub output: Option<OutputMode>,
    /// Log level override.
    pub log: Option<LogLevel>,
    /// Color mode override.
    pub color: Option<ColorMode>,
}

/// Parse the dispatcher's meta-command surface from `args`.
///
/// `args` should start with the program name (`argv\[0\]`) for parity with
/// `std::env::args_os()`.
#[must_use]
pub fn parse(mut args: impl Iterator<Item = OsString>) -> ParsedArgs {
    drop(args.next()); // skip program name

    let mut globals = GlobalFlags::default();
    let mut list_seen = false;
    let mut list_all = false;

    while let Some(arg) = args.next() {
        if arg == OsStr::new("--help") || arg == OsStr::new("-h") {
            return ParsedArgs::Help;
        }
        if arg == OsStr::new("--version") || arg == OsStr::new("-V") {
            return ParsedArgs::Version;
        }
        if arg == OsStr::new("--list") {
            list_seen = true;
            continue;
        }
        if list_seen && arg == OsStr::new("--all") {
            list_all = true;
            continue;
        }
        if arg == OsStr::new("--quiet") {
            globals.log = Some(LogLevel::Quiet);
            continue;
        }
        if arg == OsStr::new("--verbose") {
            globals.log = Some(LogLevel::Verbose);
            continue;
        }
        if arg == OsStr::new("--json") {
            globals.output = Some(OutputMode::Json);
            continue;
        }
        if arg == OsStr::new("--color") {
            if let Some(value) = args.next() {
                globals.color = Some(match value.to_string_lossy().as_ref() {
                    "always" => ColorMode::Always,
                    "never" => ColorMode::Never,
                    _ => ColorMode::Auto,
                });
            }
            continue;
        }

        // Anything else: treat as the subcommand name.
        if list_seen {
            // `--list <subcommand>` is unusual; fall through to subcommand.
        }
        let name = arg.to_string_lossy().into_owned();
        let rest: Vec<OsString> = args.collect();
        return ParsedArgs::Subcommand {
            name,
            args: rest,
            globals,
        };
    }

    if list_seen {
        return ParsedArgs::List {
            all: list_all,
            globals,
        };
    }
    ParsedArgs::Empty { globals }
}

#[cfg(test)]
#[allow(clippy::panic, clippy::missing_panics_doc)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> ParsedArgs {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_help() {
        assert!(matches!(
            parse_str(&["ready-set", "--help"]),
            ParsedArgs::Help
        ));
        assert!(matches!(parse_str(&["ready-set", "-h"]), ParsedArgs::Help));
    }

    #[test]
    fn parses_version() {
        assert!(matches!(
            parse_str(&["ready-set", "--version"]),
            ParsedArgs::Version
        ));
        assert!(matches!(
            parse_str(&["ready-set", "-V"]),
            ParsedArgs::Version
        ));
    }

    #[test]
    fn parses_list_with_all() {
        let ParsedArgs::List { all, .. } = parse_str(&["ready-set", "--list", "--all"]) else {
            panic!("expected List variant");
        };
        assert!(all);
    }

    #[test]
    fn list_carries_global_flags() {
        let ParsedArgs::List { globals, .. } = parse_str(&["ready-set", "--json", "--list"]) else {
            panic!("expected List variant");
        };
        assert_eq!(globals.output, Some(OutputMode::Json));
    }

    #[test]
    fn parses_subcommand_with_passthrough() {
        let ParsedArgs::Subcommand { name, args, .. } =
            parse_str(&["ready-set", "scan", "--json", "--path", "/tmp"])
        else {
            panic!("expected Subcommand variant");
        };
        assert_eq!(name, "scan");
        assert_eq!(
            args,
            vec![
                OsString::from("--json"),
                OsString::from("--path"),
                OsString::from("/tmp"),
            ]
        );
    }

    #[test]
    fn captures_global_flags_before_subcommand() {
        let ParsedArgs::Subcommand { name, globals, .. } =
            parse_str(&["ready-set", "--json", "--verbose", "go"])
        else {
            panic!("expected Subcommand variant");
        };
        assert_eq!(name, "go");
        assert_eq!(globals.output, Some(OutputMode::Json));
        assert_eq!(globals.log, Some(LogLevel::Verbose));
    }
}
