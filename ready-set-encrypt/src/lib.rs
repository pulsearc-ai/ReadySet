//! Secrets capability provider plugin for `ready-set`.
//!
//! Contributes three capabilities:
//! - `secrets` (ready/set/go): inventory env vars, scaffold `.env.example`, a
//!   canonical fake-value template, and `.gitleaks.toml`, run a leak scan.
//! - `rotation` (`ready-set rotate`): track per-secret rotation cadence via a manifest
//!   and an append-only audit log; rotate self-issued / exec secrets and
//!   remind on manual ones. Exec-backed rotations are wrapped in
//!   `sandbox-exec` to bound their filesystem write blast radius.
//! - `secret-bundles` (`ready-set encrypt`): encrypt configured dotenv files into
//!   `ReadySet` secret bundle files and verify that those bundles decrypt.
//!
//! This crate ships sandboxing backends for macOS (`sandbox-exec`), Linux
//! (`bubblewrap`), and Windows (`AppContainer` + per-path ACL grants via a
//! launcher binary). Other targets fail to build until a backend exists.
//!
//! The Windows backend is the only code in this crate that uses
//! `unsafe` (the Win32 FFI declarations in [`win_ffi`]). The workspace
//! lint `unsafe_code = "deny"` blocks unsafe everywhere else by
//! default; `win_ffi.rs` opts in with a file-scoped
//! `#![allow(unsafe_code)]`.

#![warn(missing_docs)]

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
compile_error!(
    "ready-set-encrypt requires a supported sandbox backend (macOS sandbox-exec, \
     Linux bubblewrap, or Windows AppContainer). To build the rest of the \
     workspace on an unsupported target, pass `--exclude ready-set-encrypt` to \
     your cargo command."
);

pub mod bundle;
mod bundle_cli;
mod config;
mod exec;
mod inventory;
mod manifest;
mod options;
mod readiness;
mod rotation;
mod runner;
mod sandbox;
mod scaffold;
mod webhook;
#[cfg(target_os = "windows")]
#[doc(hidden)]
pub mod win_ffi;
mod workflow;

use std::ffi::OsString;

use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::{
    CapabilityDescriptor, CapabilityRelevance, CapabilityVerb, CommandAlias, CommandAliasTarget,
    Context, ExitCode, LifecycleRequest, Output, ProviderId,
};

/// Provider id used by this plugin's capability descriptors.
pub const PROVIDER_ID: &str = "encrypt";

/// `.gitignore` managed-block opening marker. Namespaced to coexist with
/// `ready-set-rust`'s generic `# >>> ready-set managed >>>` marker.
pub const GITIGNORE_BEGIN: &str = "# >>> ready-set-encrypt managed >>>";

/// `.gitignore` managed-block closing marker.
pub const GITIGNORE_END: &str = "# <<< ready-set-encrypt managed <<<";

/// Return the plugin metadata payload.
#[must_use]
pub fn describe() -> Describe {
    Describe {
        description: "Secrets inventory, leak scan, sandboxed rotation".into(),
        version: env!("CARGO_PKG_VERSION")
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        stability: Stability::Experimental,
        min_dispatcher_version: "0.1.0"
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 1, 0)),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        project_requirements: Vec::new(),
        capabilities: secrets_capabilities(),
        command_aliases: command_aliases(),
    }
}

/// Capability descriptors contributed by this plugin.
#[must_use]
pub fn secrets_capabilities() -> Vec<CapabilityDescriptor> {
    vec![
        CapabilityDescriptor {
            id: "secrets".into(),
            title: "Secrets".into(),
            provider: ProviderId::from(PROVIDER_ID),
            verbs: vec![
                CapabilityVerb::Ready,
                CapabilityVerb::Set,
                CapabilityVerb::Go,
            ],
            default_relevance: CapabilityRelevance::Required,
        },
        CapabilityDescriptor {
            id: "rotation".into(),
            title: "Rotation".into(),
            provider: ProviderId::from(PROVIDER_ID),
            verbs: vec![CapabilityVerb::Ready, CapabilityVerb::Go],
            default_relevance: CapabilityRelevance::Required,
        },
        CapabilityDescriptor {
            id: "secret-bundles".into(),
            title: "Secret Bundles".into(),
            provider: ProviderId::from(PROVIDER_ID),
            verbs: vec![
                CapabilityVerb::Ready,
                CapabilityVerb::Set,
                CapabilityVerb::Go,
            ],
            default_relevance: CapabilityRelevance::Required,
        },
    ]
}

/// User-facing command aliases contributed by this plugin.
#[must_use]
pub fn command_aliases() -> Vec<CommandAlias> {
    vec![
        CommandAlias {
            name: "encrypt".into(),
            description: "Encrypt configured dotenv files into ReadySet secret bundles.".into(),
            match_first_arg: None,
            target: CommandAliasTarget::Set {
                capability: "secret-bundles".into(),
            },
        },
        CommandAlias {
            name: "encrypt".into(),
            description: "Show configured ReadySet bundles and redacted captured keys.".into(),
            match_first_arg: Some("status".into()),
            target: CommandAliasTarget::Plugin {
                args: vec!["bundle".into()],
            },
        },
        CommandAlias {
            name: "encrypt".into(),
            description: "Run a command with configured ReadySet bundle variables.".into(),
            match_first_arg: Some("exec".into()),
            target: CommandAliasTarget::Plugin { args: Vec::new() },
        },
        CommandAlias {
            name: "encrypt".into(),
            description: "Inspect and operate on ReadySet secret bundles.".into(),
            match_first_arg: Some("bundle".into()),
            target: CommandAliasTarget::Plugin { args: Vec::new() },
        },
        CommandAlias {
            name: "rotate".into(),
            description: "Rotate or record reminders for due secrets.".into(),
            match_first_arg: None,
            target: CommandAliasTarget::Go {
                capability: "rotation".into(),
            },
        },
    ]
}

/// Run the provider's direct bundle command surface.
#[must_use]
pub fn run_direct(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    bundle_cli::run(args)
}

/// Dispatch one parsed lifecycle request.
#[must_use]
pub fn run_lifecycle_request(ctx: &Context, request: LifecycleRequest) -> ExitCode {
    match request {
        LifecycleRequest::Ready { capability } => run_ready(ctx, capability.as_str()),
        LifecycleRequest::Set { capability, args } => run_set(ctx, capability.as_str(), &args),
        LifecycleRequest::Go { capability, args } => run_go(ctx, capability.as_str(), &args),
    }
}

fn run_ready(ctx: &Context, capability: &str) -> ExitCode {
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-encrypt: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let report = match readiness::report(capability, &root) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("ready-set-encrypt: {err}");
            return (&err).into();
        },
    };
    let mut out = Output::for_context(ctx, std::io::stdout());
    match out.json(&report) {
        Ok(()) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-encrypt: {err}");
            (&err).into()
        },
    }
}

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
            eprintln!("ready-set-encrypt: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };

    match runner::set_capability(capability, &root, &opts, ctx) {
        Ok(_report) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-encrypt: {err}");
            (&err).into()
        },
    }
}

fn run_go(ctx: &Context, capability: &str, args: &[OsString]) -> ExitCode {
    if !matches!(capability, "secrets" | "rotation" | "secret-bundles") {
        eprintln!("ready-set-encrypt: capability `{capability}` does not support go");
        return ExitCode::UserError;
    }
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-encrypt: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };

    match workflow::go_capability(capability, &root, ctx, args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ready-set-encrypt: {err}");
            (&err).into()
        },
    }
}
