//! `ReadySet` auth readiness provider.
//!
//! This package is local tooling for `ready-set auth`: readiness checks,
//! implementation planning, and provider metadata. Deployed applications
//! should not depend on this crate; production auth code should live in the
//! application or in an explicitly selected application dependency.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

#[cfg(feature = "plugin")]
mod options;
#[cfg(feature = "plugin")]
mod readiness;
#[cfg(feature = "plugin")]
mod runner;
#[cfg(feature = "plugin")]
mod workflow;

#[cfg(feature = "plugin")]
use std::ffi::OsString;

#[cfg(feature = "plugin")]
use ready_set_sdk::describe::{Describe, Platform, Stability};
#[cfg(feature = "plugin")]
use ready_set_sdk::{
    CapabilityDescriptor, CapabilityRelevance, CapabilityVerb, Context, ExitCode, LifecycleRequest,
    Output, ProviderId,
};

/// Provider id used by the Ready Set auth plugin.
#[cfg(feature = "plugin")]
pub const PROVIDER_ID: &str = "auth";

/// Capability id owned by this plugin.
#[cfg(feature = "plugin")]
pub const CAPABILITY_ID: &str = "auth";

/// Human title for the auth capability.
#[cfg(feature = "plugin")]
pub const CAPABILITY_TITLE: &str = "Auth";

/// Return true when `id` names the auth capability.
#[cfg(feature = "plugin")]
#[must_use]
pub fn is_auth_capability(id: &str) -> bool {
    id == CAPABILITY_ID
}

/// Return the plugin metadata payload.
#[cfg(feature = "plugin")]
#[must_use]
pub fn describe() -> Describe {
    Describe {
        description: "Web auth and OAuth readiness".into(),
        version: env!("CARGO_PKG_VERSION")
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        stability: Stability::Experimental,
        min_dispatcher_version: "0.1.0"
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 1, 0)),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        project_requirements: Vec::new(),
        capabilities: auth_capabilities(),
        command_aliases: Vec::new(),
    }
}

/// Capability descriptors contributed by this plugin.
#[cfg(feature = "plugin")]
#[must_use]
pub fn auth_capabilities() -> Vec<CapabilityDescriptor> {
    vec![CapabilityDescriptor {
        id: CAPABILITY_ID.into(),
        title: CAPABILITY_TITLE.into(),
        provider: ProviderId::from(PROVIDER_ID),
        verbs: vec![
            CapabilityVerb::Ready,
            CapabilityVerb::Set,
            CapabilityVerb::Go,
        ],
        default_relevance: CapabilityRelevance::Required,
    }]
}

/// Run the provider's direct command surface.
#[cfg(feature = "plugin")]
#[must_use]
pub fn run_direct(ctx: &Context, args: &[OsString]) -> ExitCode {
    if args.iter().any(|arg| {
        arg.to_str()
            .is_some_and(|raw| raw == "--help" || raw == "-h")
    }) {
        print_usage();
        return ExitCode::Ok;
    }
    if !args.is_empty() {
        eprintln!("ready-set-auth: unexpected arguments");
        print_usage();
        return ExitCode::UserError;
    }

    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-auth: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    match workflow::go_capability(CAPABILITY_ID, &root, ctx, &[]) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            (&err).into()
        },
    }
}

/// Dispatch one parsed lifecycle request.
#[cfg(feature = "plugin")]
#[must_use]
pub fn run_lifecycle_request(ctx: &Context, request: LifecycleRequest) -> ExitCode {
    match request {
        LifecycleRequest::Ready { capability } => run_ready(ctx, capability.as_str()),
        LifecycleRequest::Set { capability, args } => run_set(ctx, capability.as_str(), &args),
        LifecycleRequest::Go { capability, args } => run_go(ctx, capability.as_str(), &args),
    }
}

#[cfg(feature = "plugin")]
fn print_usage() {
    eprintln!(
        "ready-set-auth\n\n\
         Direct use:\n  ready-set auth\n\n\
         Lifecycle protocol:\n  ready-set-auth __ready auth\n  \
         ready-set-auth __set auth [--dry-run] [--force]\n  \
         ready-set-auth __go auth"
    );
}

#[cfg(feature = "plugin")]
fn run_ready(ctx: &Context, capability: &str) -> ExitCode {
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-auth: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let report = match readiness::report(capability, &root) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            return (&err).into();
        },
    };
    let mut out = Output::for_context(ctx, std::io::stdout());
    match out.json(&report) {
        Ok(()) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            (&err).into()
        },
    }
}

#[cfg(feature = "plugin")]
fn run_set(ctx: &Context, capability: &str, args: &[OsString]) -> ExitCode {
    let opts = match options::SetOptions::parse_args(args) {
        Ok(opts) => opts,
        Err(err) => {
            err.print().ok();
            return ExitCode::UserError;
        },
    };
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-auth: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };

    match runner::set_capability(capability, &root, &opts, ctx) {
        Ok(_report) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            (&err).into()
        },
    }
}

#[cfg(feature = "plugin")]
fn run_go(ctx: &Context, capability: &str, args: &[OsString]) -> ExitCode {
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-auth: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };

    match workflow::go_capability(capability, &root, ctx, args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            (&err).into()
        },
    }
}
