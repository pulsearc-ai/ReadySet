//! Workflow execution for Rust capabilities.

use std::process::{Command, ExitStatus, Stdio};

use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRunReport, CapabilityVerb, Context, Error,
    ExitCode, Output, OutputMode, Result, RunStatus,
};

use crate::workspace::Workspace;

/// Execute `go` for one Rust capability.
///
/// # Errors
///
/// Returns SDK errors when `cargo` cannot be spawned or JSON output cannot be
/// written.
pub fn go_capability(capability: &str, workspace: &Workspace, ctx: &Context) -> Result<ExitCode> {
    let Some(args) = cargo_args(capability) else {
        return Ok(ExitCode::UserError);
    };
    let capture_json = matches!(ctx.output_mode(), OutputMode::Json);

    if capture_json {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(&workspace.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(map_cargo_spawn_error)?;
        let report = report_for_status(capability, args, output.status);
        let exit_code = workflow_exit_code(output.status);
        let mut out = Output::for_context(ctx, std::io::stdout());
        out.json(&report)?;
        Ok(exit_code)
    } else {
        let status = Command::new("cargo")
            .args(args)
            .current_dir(&workspace.root)
            .status()
            .map_err(map_cargo_spawn_error)?;
        Ok(workflow_exit_code(status))
    }
}

fn cargo_args(capability: &str) -> Option<&'static [&'static str]> {
    match capability {
        "formatting" => Some(&["fmt", "--check"]),
        "linting" => Some(&["clippy", "--workspace", "--all-targets"]),
        _ => None,
    }
}

fn report_for_status(capability: &str, args: &[&str], status: ExitStatus) -> CapabilityRunReport {
    let command = command_label(args);
    let (run_status, action) = if status.success() {
        (
            RunStatus::Ok,
            CapabilityAction {
                kind: CapabilityActionKind::Run,
                summary: format!("{command} completed"),
                path: None,
            },
        )
    } else {
        (
            RunStatus::Failed,
            CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: format!("{command} {}", status_summary(status)),
                path: None,
            },
        )
    };

    CapabilityRunReport {
        id: capability.into(),
        verb: CapabilityVerb::Go,
        status: run_status,
        actions: vec![action],
    }
}

fn command_label(args: &[&str]) -> String {
    format!("cargo {}", args.join(" "))
}

fn status_summary(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated before reporting an exit code".into(),
        |code| format!("exited with code {code}"),
    )
}

fn workflow_exit_code(status: ExitStatus) -> ExitCode {
    if status.success() {
        ExitCode::Ok
    } else if status.code().is_some() {
        ExitCode::UserError
    } else {
        ExitCode::SystemError
    }
}

fn map_cargo_spawn_error(err: std::io::Error) -> Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        Error::MissingDependency {
            name: "cargo".into(),
            hint: Some("install Rust and Cargo, or ensure cargo is on PATH".into()),
        }
    } else {
        Error::Io(err)
    }
}
