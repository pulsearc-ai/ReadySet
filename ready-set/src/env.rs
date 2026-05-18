//! Export of the dispatcher → plugin environment contract.
//!
//! See `docs/contracts/env-vars.md`. The dispatcher SHOULD clear unknown
//! `READY_SET_*` variables from the parent environment before exec to avoid
//! injection from the calling shell.

use std::path::Path;
use std::process::Command;

use ready_set_sdk::OutputMode;
use ready_set_sdk::context::{ColorMode, LogLevel};

/// Settings exported as the dispatcher → plugin env contract.
#[derive(Debug, Clone)]
pub struct EnvContract {
    /// Dispatcher version (this binary's `CARGO_PKG_VERSION`).
    pub dispatcher_version: semver::Version,
    /// Resolved project root, if any.
    pub project_root: Option<std::path::PathBuf>,
    /// Resolved `.ready-set.toml`, if any.
    pub config_path: Option<std::path::PathBuf>,
    /// Output mode.
    pub output: OutputMode,
    /// Log level.
    pub log: LogLevel,
    /// Color preference.
    pub color: ColorMode,
}

/// Names of the env vars the dispatcher owns.
const KNOWN_VARS: &[&str] = &[
    "READY_SET_DISPATCHER_VERSION",
    "READY_SET_PROJECT_ROOT",
    "READY_SET_CONFIG_PATH",
    "READY_SET_OUTPUT",
    "READY_SET_LOG",
    "READY_SET_COLOR",
];

/// Apply the env contract to `cmd`. Clears any pre-existing `READY_SET_*`
/// variable from the child env that this function does not explicitly set,
/// so plugins never see stale values from the calling shell.
pub fn export_contract(cmd: &mut Command, contract: &EnvContract) {
    // Clear unknown READY_SET_* vars from the inherited parent env.
    for (key, _) in std::env::vars_os().filter(|(k, _)| {
        k.to_string_lossy().starts_with("READY_SET_")
            && !KNOWN_VARS.contains(&k.to_string_lossy().as_ref())
    }) {
        cmd.env_remove(&key);
    }

    cmd.env(
        "READY_SET_DISPATCHER_VERSION",
        contract.dispatcher_version.to_string(),
    );
    if let Some(root) = &contract.project_root {
        cmd.env("READY_SET_PROJECT_ROOT", absolute_or_self(root));
    } else {
        cmd.env_remove("READY_SET_PROJECT_ROOT");
    }
    if let Some(path) = &contract.config_path {
        cmd.env("READY_SET_CONFIG_PATH", absolute_or_self(path));
    } else {
        cmd.env_remove("READY_SET_CONFIG_PATH");
    }
    cmd.env(
        "READY_SET_OUTPUT",
        match contract.output {
            OutputMode::Human => "human",
            OutputMode::Json => "json",
        },
    );
    cmd.env(
        "READY_SET_LOG",
        match contract.log {
            LogLevel::Quiet => "quiet",
            LogLevel::Normal => "normal",
            LogLevel::Verbose => "verbose",
        },
    );
    cmd.env(
        "READY_SET_COLOR",
        match contract.color {
            ColorMode::Auto => "auto",
            ColorMode::Always => "always",
            ColorMode::Never => "never",
        },
    );
}

fn absolute_or_self(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
