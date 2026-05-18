//! `ready-set ready`: read-only capability diagnosis.

use std::ffi::OsString;

use ready_set_sdk::{
    CapabilityRelevance, CapabilityReport, CapabilityState, CapabilityVerb, ExitCode, OutputMode,
};

use crate::capabilities::{
    CapabilityRegistry, RegisteredCapability, render_human_matrix, render_json_matrix,
};
use crate::env::EnvContract;
use crate::lifecycle::{ReadyInvocation, invoke_ready};

/// Built-in entry point. The dispatcher routes here for `ready-set ready`.
pub fn run(args: &[OsString], contract: &EnvContract) -> ExitCode {
    let selected = match parse_selected(args) {
        Ok(selected) => selected,
        Err(code) => return code,
    };
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("ready-set ready: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };

    let registry = match CapabilityRegistry::discover(&cwd) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("ready-set ready: {err}");
            return (&err).into();
        },
    };

    let capabilities = match select_capabilities(&registry, selected.as_deref()) {
        Ok(capabilities) => capabilities,
        Err(code) => return code,
    };

    let mut reports = Vec::new();
    for capability in capabilities {
        match build_report(capability, contract) {
            Ok(report) => reports.push(report),
            Err(err) => {
                eprintln!("ready-set ready: {err}");
                return ExitCode::SystemError;
            },
        }
    }

    match contract.output {
        OutputMode::Json => match render_json_matrix(&reports) {
            Ok(json) => println!("{json}"),
            Err(err) => {
                eprintln!("ready-set ready: failed to serialize JSON report: {err}");
                return ExitCode::SystemError;
            },
        },
        OutputMode::Human => print!("{}", render_human_matrix(&reports)),
    }

    if reports.iter().any(required_report_failed) {
        ExitCode::UserError
    } else {
        ExitCode::Ok
    }
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
            eprintln!("ready-set ready: unknown capability `{id}`");
            return Err(ExitCode::UserError);
        };
        if !capability.verbs.contains(&CapabilityVerb::Ready) {
            eprintln!("ready-set ready: capability `{id}` does not support ready");
            return Err(ExitCode::UserError);
        }
        return Ok(vec![capability]);
    }

    Ok(registry.capabilities().iter().collect())
}

fn parse_selected(args: &[OsString]) -> Result<Option<String>, ExitCode> {
    match args {
        [] => Ok(None),
        [capability] => Ok(Some(capability.to_string_lossy().into_owned())),
        _ => {
            eprintln!("ready-set ready: expected at most one capability");
            Err(ExitCode::UserError)
        },
    }
}

fn build_report(
    capability: &RegisteredCapability,
    contract: &EnvContract,
) -> std::io::Result<CapabilityReport> {
    if capability.relevance == CapabilityRelevance::NotNeeded {
        return Ok(placeholder_report(
            capability,
            CapabilityState::NotNeeded,
            "capability marked not needed",
        ));
    }
    if !capability.verbs.contains(&CapabilityVerb::Ready) {
        return Ok(placeholder_report(
            capability,
            unavailable_state(capability.relevance),
            "capability does not support ready",
        ));
    }

    match invoke_ready(&capability.provider, capability.id.as_str(), contract)? {
        ReadyInvocation::Report(report) => Ok(apply_registry_metadata(capability, report)),
        ReadyInvocation::ProviderUnavailable { summary }
        | ReadyInvocation::ProviderFailed { summary } => Ok(placeholder_report(
            capability,
            unavailable_state(capability.relevance),
            summary,
        )),
    }
}

fn apply_registry_metadata(
    capability: &RegisteredCapability,
    mut report: CapabilityReport,
) -> CapabilityReport {
    report.id = capability.id.clone();
    capability.title.clone_into(&mut report.title);
    report.provider = capability.provider.clone();
    report.relevance = capability.relevance;
    if capability.relevance == CapabilityRelevance::Optional
        && !matches!(
            report.state,
            CapabilityState::Ready | CapabilityState::NotNeeded
        )
    {
        report.state = CapabilityState::Optional;
    }
    report
}

fn placeholder_report(
    capability: &RegisteredCapability,
    state: CapabilityState,
    summary: impl Into<String>,
) -> CapabilityReport {
    CapabilityReport {
        id: capability.id.clone(),
        title: capability.title.clone(),
        provider: capability.provider.clone(),
        state,
        relevance: capability.relevance,
        summary: summary.into(),
        next_action: None,
    }
}

const fn unavailable_state(relevance: CapabilityRelevance) -> CapabilityState {
    match relevance {
        CapabilityRelevance::Required => CapabilityState::Blocked,
        CapabilityRelevance::Optional => CapabilityState::Optional,
        CapabilityRelevance::NotNeeded => CapabilityState::NotNeeded,
    }
}

fn required_report_failed(report: &CapabilityReport) -> bool {
    report.relevance == CapabilityRelevance::Required
        && !matches!(
            report.state,
            CapabilityState::Ready | CapabilityState::Optional | CapabilityState::NotNeeded
        )
}

#[cfg(test)]
mod tests {
    use ready_set_sdk::{CapabilityId, CapabilityVerb, ProviderId};

    use super::*;

    fn capability(relevance: CapabilityRelevance) -> RegisteredCapability {
        RegisteredCapability {
            id: CapabilityId::from("formatting"),
            title: "Formatting".into(),
            provider: ProviderId::from("missing-provider"),
            verbs: vec![CapabilityVerb::Ready],
            relevance,
        }
    }

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

    fn descriptor(id: &str, verbs: Vec<CapabilityVerb>) -> ready_set_sdk::CapabilityDescriptor {
        ready_set_sdk::CapabilityDescriptor {
            id: id.into(),
            title: id.into(),
            provider: "provider".into(),
            verbs,
            default_relevance: CapabilityRelevance::Required,
        }
    }

    #[test]
    fn required_unavailable_provider_fails_readiness() {
        let report = placeholder_report(
            &capability(CapabilityRelevance::Required),
            unavailable_state(CapabilityRelevance::Required),
            "provider missing",
        );

        assert_eq!(report.state, CapabilityState::Blocked);
        assert!(required_report_failed(&report));
    }

    #[test]
    fn optional_unavailable_provider_does_not_fail_readiness() {
        let report = placeholder_report(
            &capability(CapabilityRelevance::Optional),
            unavailable_state(CapabilityRelevance::Optional),
            "provider missing",
        );

        assert_eq!(report.state, CapabilityState::Optional);
        assert!(!required_report_failed(&report));
    }

    #[test]
    fn not_needed_short_circuits_provider_lookup() {
        let report = placeholder_report(
            &capability(CapabilityRelevance::NotNeeded),
            CapabilityState::NotNeeded,
            "capability marked not needed",
        );

        assert_eq!(report.state, CapabilityState::NotNeeded);
        assert!(!required_report_failed(&report));
    }

    #[test]
    fn explicit_capability_must_support_ready() {
        let registry = CapabilityRegistry::from_parts(
            None,
            [manifest(vec![descriptor(
                "setup",
                vec![CapabilityVerb::Set],
            )])],
        );

        assert!(select_capabilities(&registry, Some("setup")).is_err());
    }

    #[test]
    fn readyless_capability_reports_blocked_without_invocation() {
        let capability = RegisteredCapability {
            id: CapabilityId::from("setup"),
            title: "Setup".into(),
            provider: ProviderId::from("missing-provider"),
            verbs: vec![CapabilityVerb::Set],
            relevance: CapabilityRelevance::Required,
        };

        let contract = EnvContract {
            dispatcher_version: semver::Version::new(0, 1, 0),
            project_root: None,
            config_path: None,
            output: OutputMode::Human,
            log: ready_set_sdk::context::LogLevel::Normal,
            color: ready_set_sdk::context::ColorMode::Auto,
        };
        let report = build_report(&capability, &contract).unwrap();

        assert_eq!(report.state, CapabilityState::Blocked);
        assert_eq!(report.summary, "capability does not support ready");
    }
}
