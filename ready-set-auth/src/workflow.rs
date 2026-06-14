//! Workflow execution for auth readiness audits.

use std::ffi::OsString;
use std::path::Path;

use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRunReport, CapabilityVerb, Context, Error,
    ExitCode, Output, OutputMode, Result, RunStatus,
};

use crate::readiness;
use crate::{CAPABILITY_ID, PROVIDER_ID, is_auth_capability};

/// Execute `go` for the auth capability.
///
/// # Errors
///
/// Returns SDK errors when JSON output cannot be written.
pub fn go_capability(
    capability: &str,
    root: &Path,
    ctx: &Context,
    args: &[OsString],
) -> Result<ExitCode> {
    if !is_auth_capability(capability) {
        return Err(Error::contract(format!(
            "unknown capability `{capability}` for provider `{PROVIDER_ID}`"
        )));
    }
    if !args.is_empty() {
        eprintln!("ready-set-auth: __go auth does not accept arguments");
        return Ok(ExitCode::UserError);
    }

    let audit = readiness::audit_project(root);
    let report = if audit.recognized_project {
        let ready = audit.required_ready();
        CapabilityRunReport {
            id: CAPABILITY_ID.into(),
            verb: CapabilityVerb::Go,
            status: if ready {
                RunStatus::Ok
            } else {
                RunStatus::Failed
            },
            actions: audit.to_actions(),
        }
    } else {
        CapabilityRunReport {
            id: CAPABILITY_ID.into(),
            verb: CapabilityVerb::Go,
            status: RunStatus::Noop,
            actions: vec![CapabilityAction {
                kind: CapabilityActionKind::Skip,
                summary: "no backend/login auth surface detected".into(),
                path: None,
            }],
        }
    };

    render_report(&report, ctx)?;
    if report.status == RunStatus::Failed {
        Ok(ExitCode::UserError)
    } else {
        Ok(ExitCode::Ok)
    }
}

fn render_report(report: &CapabilityRunReport, ctx: &Context) -> Result<()> {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        out.json(report)?;
        return Ok(());
    }

    out.human("ready-set-auth go auth");
    for action in &report.actions {
        let path = action.path.as_deref().unwrap_or("-");
        out.human(&format!(
            "  {:<8} {:<56} {}",
            action_kind_label(action.kind),
            path,
            action.summary
        ));
    }
    Ok(())
}

const fn action_kind_label(kind: CapabilityActionKind) -> &'static str {
    match kind {
        CapabilityActionKind::Create => "create",
        CapabilityActionKind::Modify => "modify",
        CapabilityActionKind::Delete => "delete",
        CapabilityActionKind::Run => "run",
        CapabilityActionKind::Check => "check",
        CapabilityActionKind::Skip => "skip",
        CapabilityActionKind::Error => "error",
    }
}
