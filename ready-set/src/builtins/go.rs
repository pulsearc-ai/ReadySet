//! `ready-set go`: provider-backed workflow execution.

use std::ffi::OsString;

use clap::Parser;
use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRelevance, CapabilityRunReport,
    CapabilityVerb, ExitCode, OutputMode, RunStatus,
};

use crate::capabilities::{CapabilityRegistry, RegisteredCapability};
use crate::env::EnvContract;
use crate::lifecycle::{GoInvocation, invoke_go};

/// Options accepted by `ready-set go`.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set go", about, long_about = None, no_binary_name = true)]
struct Options {
    /// Capability id to run.
    pub capability: Option<String>,

    /// Emit machine-readable JSON output.
    #[arg(long)]
    pub json: bool,

    /// Errors only.
    #[arg(long)]
    pub quiet: bool,

    /// Debug logging.
    #[arg(long)]
    pub verbose: bool,
}

/// Built-in entry point. The dispatcher routes here for `ready-set go`.
pub fn run(args: &[OsString], contract: &EnvContract) -> ExitCode {
    let opts = match Options::try_parse_from(args) {
        Ok(opts) => opts,
        Err(err) => {
            err.print().ok();
            return ExitCode::UserError;
        },
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("ready-set go: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let registry = match CapabilityRegistry::discover(&cwd) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("ready-set go: {err}");
            return (&err).into();
        },
    };

    let capabilities = match select_capabilities(&registry, opts.capability.as_deref()) {
        Ok(capabilities) => capabilities,
        Err(code) => return code,
    };
    if capabilities.is_empty() {
        eprintln!("ready-set go: no required go-capable capabilities found");
        return ExitCode::UserError;
    }

    let capture_json = matches!(contract.output, OutputMode::Json) || opts.json;
    let mut reports = Vec::new();
    let mut exit_code = ExitCode::Ok;

    for capability in capabilities {
        if capability.relevance == CapabilityRelevance::NotNeeded {
            reports.push(noop_report(capability, "capability marked not needed"));
            continue;
        }

        match invoke_go(
            &capability.provider,
            capability.id.as_str(),
            &[],
            contract,
            capture_json,
        ) {
            Ok(GoInvocation::Report {
                report,
                exit_code: code,
            }) => {
                if code != ExitCode::Ok {
                    exit_code = merge_exit_codes(exit_code, code);
                } else if report.status == RunStatus::Failed {
                    exit_code = merge_exit_codes(exit_code, ExitCode::UserError);
                }
                reports.push(report);
            },
            Ok(GoInvocation::Streamed { exit_code: code }) => {
                exit_code = merge_exit_codes(exit_code, code);
            },
            Ok(GoInvocation::ProviderUnavailable { summary }) => {
                eprintln!("ready-set go: {summary}");
                exit_code = merge_exit_codes(exit_code, ExitCode::UserError);
                if capture_json {
                    reports.push(failed_report(capability, &summary));
                }
            },
            Ok(GoInvocation::ProviderFailed {
                exit_code: code,
                summary,
            }) => {
                eprintln!("ready-set go: {summary}");
                exit_code = merge_exit_codes(exit_code, code);
                if capture_json {
                    reports.push(failed_report(capability, &summary));
                }
            },
            Err(err) => {
                let summary = err.to_string();
                eprintln!("ready-set go: {summary}");
                exit_code = merge_exit_codes(exit_code, ExitCode::SystemError);
                if capture_json {
                    reports.push(failed_report(capability, &summary));
                }
            },
        }
    }

    if capture_json {
        match serde_json::to_string(&reports) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("ready-set go: failed to serialize JSON report: {err}");
                return ExitCode::SystemError;
            },
        }
    }

    exit_code
}

fn select_capabilities<'a>(
    registry: &'a CapabilityRegistry,
    selected: Option<&str>,
) -> Result<Vec<&'a RegisteredCapability>, ExitCode> {
    if let Some(id) = selected {
        let Some(capability) = registry
            .capabilities()
            .iter()
            .find(|capability| capability.id.as_str() == id)
        else {
            eprintln!("ready-set go: unknown capability `{id}`");
            return Err(ExitCode::UserError);
        };
        if !capability.verbs.contains(&CapabilityVerb::Go) {
            eprintln!("ready-set go: capability `{id}` does not support go");
            return Err(ExitCode::UserError);
        }
        return Ok(vec![capability]);
    }

    Ok(registry
        .capabilities()
        .iter()
        .filter(|capability| {
            capability.relevance == CapabilityRelevance::Required
                && capability.verbs.contains(&CapabilityVerb::Go)
        })
        .collect())
}

fn noop_report(capability: &RegisteredCapability, summary: &str) -> CapabilityRunReport {
    CapabilityRunReport {
        id: capability.id.clone(),
        verb: CapabilityVerb::Go,
        status: RunStatus::Noop,
        actions: vec![CapabilityAction {
            kind: CapabilityActionKind::Skip,
            summary: summary.into(),
            path: None,
        }],
    }
}

fn failed_report(capability: &RegisteredCapability, summary: &str) -> CapabilityRunReport {
    CapabilityRunReport {
        id: capability.id.clone(),
        verb: CapabilityVerb::Go,
        status: RunStatus::Failed,
        actions: vec![CapabilityAction {
            kind: CapabilityActionKind::Error,
            summary: summary.into(),
            path: None,
        }],
    }
}

const fn merge_exit_codes(current: ExitCode, next: ExitCode) -> ExitCode {
    if next.as_u8() > current.as_u8() {
        next
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(
        capabilities: Vec<ready_set_sdk::CapabilityDescriptor>,
    ) -> ready_set_sdk::manifest::Manifest {
        ready_set_sdk::manifest::Manifest {
            description: "test".into(),
            version: "0.1.0".parse().unwrap(),
            stability: ready_set_sdk::describe::Stability::Stable,
            min_dispatcher_version: "0.1.0".parse().unwrap(),
            platforms: vec![ready_set_sdk::describe::Platform::current().unwrap()],
            requires_cargo_workspace: false,
            capabilities,
        }
    }

    fn descriptor(
        id: &str,
        relevance: CapabilityRelevance,
        verbs: Vec<CapabilityVerb>,
    ) -> ready_set_sdk::CapabilityDescriptor {
        ready_set_sdk::CapabilityDescriptor {
            id: id.into(),
            title: id.into(),
            provider: "provider".into(),
            verbs,
            default_relevance: relevance,
        }
    }

    #[test]
    fn no_capability_selects_required_go_capabilities_only() {
        let registry = CapabilityRegistry::from_parts(
            None,
            [manifest(vec![
                descriptor(
                    "formatting",
                    CapabilityRelevance::Required,
                    vec![CapabilityVerb::Ready, CapabilityVerb::Go],
                ),
                descriptor(
                    "optional",
                    CapabilityRelevance::Optional,
                    vec![CapabilityVerb::Ready, CapabilityVerb::Go],
                ),
                descriptor(
                    "setup",
                    CapabilityRelevance::Required,
                    vec![CapabilityVerb::Ready, CapabilityVerb::Set],
                ),
            ])],
        );
        let selected = select_capabilities(&registry, None).unwrap();

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id.as_str(), "formatting");
    }

    #[test]
    fn explicit_capability_must_support_go() {
        let registry = CapabilityRegistry::from_parts(
            None,
            [manifest(vec![descriptor(
                "workspace",
                CapabilityRelevance::Required,
                vec![CapabilityVerb::Ready, CapabilityVerb::Set],
            )])],
        );

        assert!(select_capabilities(&registry, Some("workspace")).is_err());
    }

    #[test]
    fn failed_report_uses_go_verb() {
        let registry = CapabilityRegistry::from_parts(
            None,
            [manifest(vec![descriptor(
                "formatting",
                CapabilityRelevance::Required,
                vec![CapabilityVerb::Ready, CapabilityVerb::Go],
            )])],
        );
        let report = failed_report(&registry.capabilities()[0], "failed");

        assert_eq!(report.verb, CapabilityVerb::Go);
        assert_eq!(report.status, RunStatus::Failed);
        assert_eq!(report.actions[0].kind, CapabilityActionKind::Error);
    }
}
