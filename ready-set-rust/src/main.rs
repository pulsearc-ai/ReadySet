//! `ready-set-rust` provider plugin entry point.

use ready_set_rust::describe;
use ready_set_rust::options::SetOptions;
use ready_set_rust::readiness;
use ready_set_rust::runner;
use ready_set_rust::workflow;
use ready_set_rust::workspace;
use ready_set_sdk::{Context, ExitCode, LifecycleRequest, Output, parse_lifecycle_request};

fn main() -> std::process::ExitCode {
    let description = describe();
    if let Some(code) = description.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }

    let request = match parse_lifecycle_request(std::env::args_os()) {
        Ok(Some(request)) => request,
        Ok(None) => {
            eprintln!("ready-set-rust is a ready-set provider plugin");
            return ExitCode::Ok.into();
        },
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            return ExitCode::UserError.into();
        },
    };

    let ctx = Context::from_env();
    match request {
        LifecycleRequest::Ready { capability } => run_ready(&ctx, capability.as_str()).into(),
        LifecycleRequest::Set { capability, args } => {
            run_set(&ctx, capability.as_str(), &args).into()
        },
        LifecycleRequest::Go { capability, args } => {
            run_go(&ctx, capability.as_str(), &args).into()
        },
    }
}

fn run_go(ctx: &Context, capability: &str, args: &[std::ffi::OsString]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("ready-set-rust: __go does not accept additional arguments");
        return ExitCode::UserError;
    }
    if !matches!(capability, "formatting" | "linting") {
        eprintln!("ready-set-rust: capability `{capability}` does not support go");
        return ExitCode::UserError;
    }
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-rust: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let workspace = match workspace::resolve(&root) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            eprintln!(
                "ready-set-rust: not a cargo workspace (no Cargo.toml with [workspace] or \
                 [package] found above {})",
                root.display()
            );
            return ExitCode::NotCargoWorkspace;
        },
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            return (&err).into();
        },
    };

    match workflow::go_capability(capability, &workspace, ctx) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            (&err).into()
        },
    }
}

fn run_ready(ctx: &Context, capability: &str) -> ExitCode {
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-rust: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let workspace = match workspace::resolve(&root) {
        Ok(workspace) => workspace,
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            return (&err).into();
        },
    };
    let report = match readiness::report(capability, workspace.as_ref()) {
        Ok(report) => report,
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            return (&err).into();
        },
    };
    let mut out = Output::for_context(ctx, std::io::stdout());
    match out.json(&report) {
        Ok(()) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            (&err).into()
        },
    }
}

fn run_set(ctx: &Context, capability: &str, args: &[std::ffi::OsString]) -> ExitCode {
    let opts = match SetOptions::parse_args(args) {
        Ok(opts) => opts,
        Err(err) => {
            err.print().ok();
            return ExitCode::UserError;
        },
    };
    let root = match ctx.project_root_or_cwd() {
        Ok(root) => root,
        Err(err) => {
            eprintln!("ready-set-rust: cannot read current directory: {err}");
            return ExitCode::SystemError;
        },
    };
    let workspace = match workspace::resolve(&root) {
        Ok(Some(workspace)) => workspace,
        Ok(None) => {
            eprintln!(
                "ready-set-rust: not a cargo workspace (no Cargo.toml with [workspace] or \
                 [package] found above {})",
                root.display()
            );
            return ExitCode::NotCargoWorkspace;
        },
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            return (&err).into();
        },
    };

    match runner::set_capability(capability, &workspace, &opts, ctx) {
        Ok(_report) => ExitCode::Ok,
        Err(err) => {
            eprintln!("ready-set-rust: {err}");
            (&err).into()
        },
    }
}
