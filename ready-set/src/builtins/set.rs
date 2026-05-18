//! `ready-set set`: provider-backed setup and reconciliation.

use std::ffi::OsString;

use clap::Parser;
use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRelevance, CapabilityRunReport,
    CapabilityVerb, ExitCode, OutputMode, RunStatus,
};

use crate::capabilities::{CapabilityRegistry, RegisteredCapability};
use crate::env::EnvContract;
use crate::lifecycle::{SetInvocation, invoke_set};

/// Options accepted by `ready-set set`.
#[derive(Debug, Clone, Parser)]
#[command(name = "ready-set set", about, long_about = None, no_binary_name = true)]
#[allow(clippy::struct_excessive_bools)]
struct Options {
    /// Capability id to reconcile.
    pub capability: Option<String>,

    /// Replace managed files even if their content has diverged.
    #[arg(long)]
    pub force: bool,

    /// Plan and report writes without modifying any files.
    #[arg(long)]
    pub dry_run: bool,

    /// Emit machine-readable JSON output.
    #[arg(long)]
    pub json: bool,

    /// Errors only.
    #[arg(long)]
    pub quiet: bool,

    /// Debug logging.
    #[arg(long)]
    pub verbose: bool,

    /// Explicit member path to add to `[workspace.members]`. Repeatable.
    #[arg(long = "member")]
    pub members: Vec<String>,

    /// Skip recursive crate discovery.
    #[arg(long)]
    pub no_discover: bool,
}

/// Built-in entry point. The dispatcher routes here for `ready-set set`.
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
            eprintln!("ready-set set: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let registry = match CapabilityRegistry::discover(&cwd) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("ready-set set: {err}");
            return (&err).into();
        },
    };

    let capabilities = match select_capabilities(&registry, opts.capability.as_deref()) {
        Ok(capabilities) => capabilities,
        Err(code) => return code,
    };
    if capabilities.is_empty() {
        eprintln!("ready-set set: no required set-capable capabilities found");
        return ExitCode::UserError;
    }

    let capture_json = matches!(contract.output, OutputMode::Json) || opts.json;
    let provider_args = provider_args(&opts);
    let mut reports = Vec::new();
    let mut exit_code = ExitCode::Ok;

    for capability in capabilities {
        if capability.relevance == CapabilityRelevance::NotNeeded {
            reports.push(noop_report(capability, "capability marked not needed"));
            continue;
        }

        match invoke_set(
            &capability.provider,
            capability.id.as_str(),
            &provider_args,
            contract,
            capture_json,
        ) {
            Ok(SetInvocation::Report(report)) => reports.push(report),
            Ok(SetInvocation::Streamed { exit_code: code }) => {
                if code != ExitCode::Ok {
                    exit_code = code;
                }
            },
            Ok(SetInvocation::ProviderUnavailable { summary }) => {
                eprintln!("ready-set set: {summary}");
                return ExitCode::UserError;
            },
            Ok(SetInvocation::ProviderFailed {
                exit_code: code,
                summary,
            }) => {
                eprintln!("ready-set set: {summary}");
                return code;
            },
            Err(err) => {
                eprintln!("ready-set set: {err}");
                return ExitCode::SystemError;
            },
        }
    }

    if capture_json {
        match serde_json::to_string(&reports) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("ready-set set: failed to serialize JSON report: {err}");
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
            eprintln!("ready-set set: unknown capability `{id}`");
            return Err(ExitCode::UserError);
        };
        if !capability.verbs.contains(&CapabilityVerb::Set) {
            eprintln!("ready-set set: capability `{id}` does not support set");
            return Err(ExitCode::UserError);
        }
        return Ok(vec![capability]);
    }

    Ok(registry
        .capabilities()
        .iter()
        .filter(|capability| {
            capability.relevance == CapabilityRelevance::Required
                && capability.verbs.contains(&CapabilityVerb::Set)
        })
        .collect())
}

fn provider_args(opts: &Options) -> Vec<OsString> {
    let mut args = Vec::new();
    if opts.dry_run {
        args.push(OsString::from("--dry-run"));
    }
    if opts.force {
        args.push(OsString::from("--force"));
    }
    if opts.no_discover {
        args.push(OsString::from("--no-discover"));
    }
    for member in &opts.members {
        args.push(OsString::from("--member"));
        args.push(OsString::from(member));
    }
    args
}

fn noop_report(capability: &RegisteredCapability, summary: &str) -> CapabilityRunReport {
    CapabilityRunReport {
        id: capability.id.clone(),
        verb: CapabilityVerb::Set,
        status: RunStatus::Noop,
        actions: vec![CapabilityAction {
            kind: CapabilityActionKind::Skip,
            summary: summary.into(),
            path: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_args_preserve_setup_flags() {
        let opts = Options {
            capability: Some("workspace".into()),
            force: true,
            dry_run: true,
            json: false,
            quiet: false,
            verbose: false,
            members: vec!["crates/foo".into()],
            no_discover: true,
        };

        assert_eq!(
            provider_args(&opts),
            vec![
                OsString::from("--dry-run"),
                OsString::from("--force"),
                OsString::from("--no-discover"),
                OsString::from("--member"),
                OsString::from("crates/foo"),
            ]
        );
    }

    #[test]
    fn no_capability_selects_required_set_capabilities_only() {
        let manifest = ready_set_sdk::manifest::Manifest {
            description: "test".into(),
            version: "0.1.0".parse().unwrap(),
            stability: ready_set_sdk::describe::Stability::Stable,
            min_dispatcher_version: "0.1.0".parse().unwrap(),
            platforms: vec![ready_set_sdk::describe::Platform::current().unwrap()],
            requires_cargo_workspace: false,
            capabilities: vec![
                ready_set_sdk::CapabilityDescriptor {
                    id: "required".into(),
                    title: "Required".into(),
                    provider: "provider".into(),
                    verbs: vec![CapabilityVerb::Ready, CapabilityVerb::Set],
                    default_relevance: CapabilityRelevance::Required,
                },
                ready_set_sdk::CapabilityDescriptor {
                    id: "optional".into(),
                    title: "Optional".into(),
                    provider: "provider".into(),
                    verbs: vec![CapabilityVerb::Ready, CapabilityVerb::Set],
                    default_relevance: CapabilityRelevance::Optional,
                },
            ],
        };
        let registry = CapabilityRegistry::from_parts(None, [manifest]);
        let selected = select_capabilities(&registry, None).unwrap();

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].id.as_str(), "required");
    }

    #[test]
    fn explicit_capability_must_support_set() {
        let manifest = ready_set_sdk::manifest::Manifest {
            description: "test".into(),
            version: "0.1.0".parse().unwrap(),
            stability: ready_set_sdk::describe::Stability::Stable,
            min_dispatcher_version: "0.1.0".parse().unwrap(),
            platforms: vec![ready_set_sdk::describe::Platform::current().unwrap()],
            requires_cargo_workspace: false,
            capabilities: vec![ready_set_sdk::CapabilityDescriptor {
                id: "ready-only".into(),
                title: "Ready only".into(),
                provider: "provider".into(),
                verbs: vec![CapabilityVerb::Ready],
                default_relevance: CapabilityRelevance::Required,
            }],
        };
        let registry = CapabilityRegistry::from_parts(None, [manifest]);

        assert!(select_capabilities(&registry, Some("ready-only")).is_err());
    }
}
