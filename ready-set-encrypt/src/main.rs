//! `ready-set-encrypt` provider plugin entry point.

use ready_set_encrypt::describe;
use ready_set_encrypt::{run_direct, run_lifecycle_request};
use ready_set_sdk::{Context, ExitCode, parse_lifecycle_request};

fn main() -> std::process::ExitCode {
    let description = describe();
    if let Some(code) = description.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }

    let request = match parse_lifecycle_request(std::env::args_os()) {
        Ok(Some(request)) => request,
        Ok(None) => {
            if std::env::args_os().len() > 1 {
                return run_direct(std::env::args_os()).into();
            }
            eprintln!("ready-set-encrypt is a ready-set provider plugin");
            return ExitCode::Ok.into();
        },
        Err(err) => {
            eprintln!("ready-set-encrypt: {err}");
            return ExitCode::UserError.into();
        },
    };

    let ctx = Context::from_env();
    run_lifecycle_request(&ctx, request).into()
}
