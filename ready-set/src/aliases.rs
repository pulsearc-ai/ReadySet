//! Provider-declared command alias resolution and dispatch.

use std::ffi::OsString;

use ready_set_sdk::describe::Platform;
use ready_set_sdk::{
    CapabilityVerb, CommandAlias, CommandAliasTarget, ExitCode, OutputMode, RunStatus,
};
use serde::Serialize;

use crate::cache::PluginCache;
use crate::capabilities::{CapabilityRegistry, RegisteredCapability};
use crate::discovery::{PluginEntry, list_all};
use crate::env::EnvContract;
use crate::exec::dispatch_to_plugin;
use crate::lifecycle::{GoInvocation, SetInvocation, invoke_go, invoke_set};
use crate::metadata::resolve_metadata;

/// A plugin-declared alias selected for the user's subcommand.
#[derive(Debug, Clone)]
pub struct ResolvedAlias {
    plugin: PluginEntry,
    alias: CommandAlias,
}

/// Resolve a user subcommand to a provider-declared command alias.
#[must_use]
pub fn resolve(name: &str, args: &[OsString]) -> Option<ResolvedAlias> {
    let cache_path = PluginCache::default_path();
    let mut cache = cache_path
        .as_deref()
        .map_or_else(PluginCache::default, PluginCache::load);
    let mut cache_dirty = false;
    let current_platform = Platform::current();
    let mut best: Option<ResolvedAlias> = None;

    for entry in list_all() {
        let Some(manifest) = resolve_metadata(&entry, &mut cache) else {
            continue;
        };
        cache_dirty = true;
        if current_platform.is_some_and(|platform| !manifest.platforms.contains(&platform)) {
            continue;
        }
        for alias in manifest.command_aliases {
            if alias.name != name || !alias.matches_args(args) {
                continue;
            }
            let candidate = ResolvedAlias {
                plugin: entry.clone(),
                alias,
            };
            let replace = best.as_ref().is_none_or(|selected| {
                candidate.alias.specificity() > selected.alias.specificity()
            });
            if replace {
                best = Some(candidate);
            }
        }
    }

    if cache_dirty && let Some(path) = cache_path.as_deref() {
        drop(cache.save(path));
    }

    best
}

/// Run a resolved provider-declared command alias.
pub fn run(resolved: &ResolvedAlias, args: &[OsString], contract: &EnvContract) -> ExitCode {
    match &resolved.alias.target {
        CommandAliasTarget::Set { capability } => {
            run_set_alias(&resolved.alias.name, capability.as_str(), args, contract)
        },
        CommandAliasTarget::Go { capability } => {
            run_go_alias(&resolved.alias.name, capability.as_str(), args, contract)
        },
        CommandAliasTarget::Plugin { args: prefix } => {
            let forwarded: Vec<OsString> = prefix
                .iter()
                .map(OsString::from)
                .chain(args.iter().cloned())
                .collect();
            dispatch_to_plugin(&resolved.plugin, &forwarded, contract)
        },
    }
}

fn run_set_alias(
    command: &str,
    capability_id: &str,
    args: &[OsString],
    contract: &EnvContract,
) -> ExitCode {
    let capability = match resolve_capability(command, capability_id, CapabilityVerb::Set) {
        Ok(capability) => capability,
        Err(code) => return code,
    };
    let capture_json = matches!(contract.output, OutputMode::Json);

    match invoke_set(
        &capability.provider,
        capability.id.as_str(),
        args,
        contract,
        capture_json,
    ) {
        Ok(SetInvocation::Report(report)) => {
            if capture_json && !emit_json(command, &report) {
                return ExitCode::SystemError;
            }
            ExitCode::Ok
        },
        Ok(SetInvocation::Streamed { exit_code }) => exit_code,
        Ok(SetInvocation::ProviderUnavailable { summary }) => {
            eprintln!("ready-set {command}: {summary}");
            ExitCode::UserError
        },
        Ok(SetInvocation::ProviderFailed { exit_code, summary }) => {
            eprintln!("ready-set {command}: {summary}");
            exit_code
        },
        Err(err) => {
            eprintln!("ready-set {command}: {err}");
            ExitCode::SystemError
        },
    }
}

fn run_go_alias(
    command: &str,
    capability_id: &str,
    args: &[OsString],
    contract: &EnvContract,
) -> ExitCode {
    let capability = match resolve_capability(command, capability_id, CapabilityVerb::Go) {
        Ok(capability) => capability,
        Err(code) => return code,
    };
    let capture_json = matches!(contract.output, OutputMode::Json);

    match invoke_go(
        &capability.provider,
        capability.id.as_str(),
        args,
        contract,
        capture_json,
    ) {
        Ok(GoInvocation::Report { report, exit_code }) => {
            if capture_json && !emit_json(command, &report) {
                return ExitCode::SystemError;
            }
            if exit_code != ExitCode::Ok {
                exit_code
            } else if report.status == RunStatus::Failed {
                ExitCode::UserError
            } else {
                ExitCode::Ok
            }
        },
        Ok(GoInvocation::Streamed { exit_code }) => exit_code,
        Ok(GoInvocation::ProviderUnavailable { summary }) => {
            eprintln!("ready-set {command}: {summary}");
            ExitCode::UserError
        },
        Ok(GoInvocation::ProviderFailed { exit_code, summary }) => {
            eprintln!("ready-set {command}: {summary}");
            exit_code
        },
        Err(err) => {
            eprintln!("ready-set {command}: {err}");
            ExitCode::SystemError
        },
    }
}

fn resolve_capability(
    command: &str,
    capability_id: &str,
    verb: CapabilityVerb,
) -> Result<RegisteredCapability, ExitCode> {
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            eprintln!("ready-set {command}: cannot read current directory: {err}");
            return Err(ExitCode::SystemError);
        },
    };
    let registry = match CapabilityRegistry::discover(&cwd) {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("ready-set {command}: {err}");
            return Err((&err).into());
        },
    };
    let Some(capability) = registry
        .capabilities()
        .iter()
        .find(|capability| capability.id.as_str() == capability_id)
    else {
        eprintln!("ready-set {command}: capability `{capability_id}` is not installed");
        return Err(ExitCode::UserError);
    };
    if !capability.verbs.contains(&verb) {
        eprintln!(
            "ready-set {command}: capability `{capability_id}` does not support {}",
            verb_label(verb)
        );
        return Err(ExitCode::UserError);
    }
    Ok(capability.clone())
}

fn emit_json<T: Serialize>(command: &str, value: &T) -> bool {
    match serde_json::to_string(value) {
        Ok(json) => {
            println!("{json}");
            true
        },
        Err(err) => {
            eprintln!("ready-set {command}: failed to serialize JSON report: {err}");
            false
        },
    }
}

const fn verb_label(verb: CapabilityVerb) -> &'static str {
    match verb {
        CapabilityVerb::Ready => "ready",
        CapabilityVerb::Set => "set",
        CapabilityVerb::Go => "go",
    }
}
