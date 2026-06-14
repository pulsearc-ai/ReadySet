//! `ready-set-auth` provider plugin entry point.

use ready_set_auth::describe;
use ready_set_auth::{run_direct, run_lifecycle_request};
use ready_set_sdk::{Context, ExitCode, parse_lifecycle_request};

fn main() -> std::process::ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let description = describe();
    if let Some(code) = description.handle_arg0_describe(args.clone()) {
        return code.into();
    }

    let request = match parse_lifecycle_request(args.clone()) {
        Ok(Some(request)) => request,
        Ok(None) => {
            let ctx = Context::from_env();
            return run_direct(&ctx, &args[1..]).into();
        },
        Err(err) => {
            eprintln!("ready-set-auth: {err}");
            return ExitCode::UserError.into();
        },
    };

    let ctx = Context::from_env();
    run_lifecycle_request(&ctx, request).into()
}
