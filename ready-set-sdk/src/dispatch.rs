//! Helpers for one plugin to dispatch to another via the `ready-set` core.
//!
//! All inter-plugin calls go through the dispatcher (not directly to a
//! `ready-set-<name>` binary) so PATH semantics and built-in resolution stay
//! consistent. The env contract is forwarded automatically.

use std::ffi::OsString;
use std::process::Command;

use crate::context::{ColorMode, Context, LogLevel};
use crate::error::{Error, Result};
use crate::output::OutputMode;

/// Where to find the dispatcher binary. Tests and integration scenarios may
/// set the `READY_SET_BIN` env var to override the default `which`-based
/// lookup.
fn locate_dispatcher() -> Result<std::path::PathBuf> {
    if let Some(explicit) = std::env::var_os("READY_SET_BIN") {
        return Ok(std::path::PathBuf::from(explicit));
    }
    which::which("ready-set").map_err(|_| Error::MissingDependency {
        name: "ready-set".to_string(),
        hint: Some("install via `cargo install ready-set`".to_string()),
    })
}

/// Outcome of a dispatched subcommand.
#[derive(Debug)]
pub enum DispatchOutcome {
    /// stdout was streamed to the parent's stdout.
    Streamed {
        /// Process exit code reported by the child.
        exit_code: i32,
    },
    /// stdout was captured for programmatic use.
    Captured {
        /// Captured stdout bytes.
        stdout: Vec<u8>,
        /// Process exit code reported by the child.
        exit_code: i32,
    },
}

/// Builder for a dispatched subcommand call.
pub struct DispatchBuilder {
    subcommand: String,
    args: Vec<OsString>,
    capture: bool,
}

impl DispatchBuilder {
    /// Start a builder for `ready-set <subcommand>`.
    #[must_use]
    pub fn new(subcommand: impl Into<String>) -> Self {
        Self {
            subcommand: subcommand.into(),
            args: Vec::new(),
            capture: false,
        }
    }

    /// Append one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append multiple arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Capture stdout instead of streaming it.
    #[must_use]
    pub const fn capture(mut self, yes: bool) -> Self {
        self.capture = yes;
        self
    }

    /// Run the dispatched subcommand.
    ///
    /// # Errors
    ///
    /// Returns [`Error::MissingDependency`] if the dispatcher binary cannot
    /// be located on PATH (override with `READY_SET_BIN`), or [`Error::Io`]
    /// if spawning the child fails.
    pub fn run(self, ctx: &Context) -> Result<DispatchOutcome> {
        let bin = locate_dispatcher()?;
        let mut cmd = Command::new(bin);
        cmd.arg(&self.subcommand);
        cmd.args(&self.args);

        // Forward env contract verbatim so the child sees what we received.
        export_contract(&mut cmd, ctx);

        if self.capture {
            cmd.stdout(std::process::Stdio::piped());
            let output = cmd.output()?;
            Ok(DispatchOutcome::Captured {
                stdout: output.stdout,
                exit_code: output.status.code().unwrap_or(-1),
            })
        } else {
            let status = cmd.status()?;
            Ok(DispatchOutcome::Streamed {
                exit_code: status.code().unwrap_or(-1),
            })
        }
    }
}

/// Export the dispatcher env contract onto `cmd` from `ctx`.
pub fn export_contract(cmd: &mut Command, ctx: &Context) {
    if let Some(version) = ctx.dispatcher_version() {
        cmd.env("READY_SET_DISPATCHER_VERSION", version.to_string());
    }
    if let Some(root) = ctx.project_root() {
        cmd.env("READY_SET_PROJECT_ROOT", root);
    }
    if let Some(cfg) = ctx.config_path() {
        cmd.env("READY_SET_CONFIG_PATH", cfg);
    }
    cmd.env(
        "READY_SET_OUTPUT",
        match ctx.output_mode() {
            OutputMode::Human => "human",
            OutputMode::Json => "json",
        },
    );
    cmd.env(
        "READY_SET_LOG",
        match ctx.log_level() {
            LogLevel::Quiet => "quiet",
            LogLevel::Normal => "normal",
            LogLevel::Verbose => "verbose",
        },
    );
    cmd.env(
        "READY_SET_COLOR",
        match ctx.color() {
            ColorMode::Auto => "auto",
            ColorMode::Always => "always",
            ColorMode::Never => "never",
        },
    );
}
