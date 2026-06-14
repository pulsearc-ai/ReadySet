//! ready-set core: dispatcher + built-ins.
//!
//! Most consumers want the `ready-set` binary, not this library. The library
//! is exposed so integration tests can drive the dispatcher in-process.
//!
//! The dispatcher↔plugin contracts the core implements are versioned and
//! live under
//! [`docs/contracts/`](https://github.com/pulsearc-ai/ReadySet/tree/main/docs/contracts):
//! [`env-vars.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/env-vars.md)
//! (the `READY_SET_*` env surface),
//! [`capabilities.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/capabilities.md)
//! (descriptor and report shapes),
//! [`describe.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/describe.md)
//! and
//! [`manifest.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/manifest.md)
//! (plugin metadata discovery),
//! [`exit-codes.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/exit-codes.md)
//! (process exit code semantics),
//! [`cache.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/cache.md)
//! (the `--list` cache).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod aliases;
pub mod builtins;
pub mod cache;
pub mod capabilities;
pub mod cli;
pub mod discovery;
pub mod env;
pub mod exec;
pub mod lifecycle;
pub mod metadata;
pub mod project;

use std::ffi::OsString;
use std::path::PathBuf;

use ready_set_sdk::context::{ColorMode, LogLevel};
use ready_set_sdk::{ExitCode, OutputMode};

/// Entry point shared by `main.rs` and integration tests.
pub fn run(argv: impl IntoIterator<Item = OsString>) -> ExitCode {
    let parsed = cli::parse(argv.into_iter());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match parsed {
        cli::ParsedArgs::Help => {
            let contract = build_contract(&cli::GlobalFlags::default(), &cwd);
            builtins::help::run(&[], &contract)
        },
        cli::ParsedArgs::Version => {
            let contract = build_contract(&cli::GlobalFlags::default(), &cwd);
            builtins::version::run(&[], &contract)
        },
        cli::ParsedArgs::List { all, globals } => {
            let contract = build_contract(&globals, &cwd);
            let forwarded: Vec<OsString> = if all {
                vec![OsString::from("--all")]
            } else {
                Vec::new()
            };
            builtins::list::run(&forwarded, &contract)
        },
        cli::ParsedArgs::Empty { globals } => {
            let contract = build_contract(&globals, &cwd);
            builtins::ready::run(&[], &contract)
        },
        cli::ParsedArgs::Subcommand {
            name,
            args,
            globals,
        } => {
            let contract = build_contract(&globals, &cwd);
            if let Some(handler) = builtins::route(&name) {
                return handler(&args, &contract);
            }
            if let Some(alias) = aliases::resolve(&name, &args) {
                return aliases::run(&alias, &args, &contract);
            }
            // Fall through to plugin discovery.
            let Some(entry) = discovery::find_plugin(&name) else {
                eprintln!(
                    "ready-set: unknown subcommand `{name}`\n\
                     hint: search crates.io for `ready-set-{name}` to install one"
                );
                return ExitCode::UnknownSubcommand;
            };
            exec::dispatch_to_plugin(&entry, &args, &contract)
        },
    }
}

fn build_contract(globals: &cli::GlobalFlags, cwd: &std::path::Path) -> env::EnvContract {
    let project_root = project::detect_project_root(cwd);
    let config_path = project_root
        .as_deref()
        .map(|root| root.join(".ready-set.toml"))
        .filter(|p| p.is_file());

    env::EnvContract {
        dispatcher_version: env!("CARGO_PKG_VERSION")
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        project_root,
        config_path,
        output: globals.output.unwrap_or(OutputMode::Human),
        log: globals.log.unwrap_or(LogLevel::Normal),
        color: globals.color.unwrap_or(ColorMode::Auto),
    }
}
