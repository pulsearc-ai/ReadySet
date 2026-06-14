//! Minimal plugin example demonstrating the canonical shape.
//!
//! Build and run as:
//!
//! ```sh
//! cargo run --example minimal_plugin -- __describe
//! READY_SET_OUTPUT=human cargo run --example minimal_plugin
//! ```

use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::prelude::*;

fn describe() -> Describe {
    Describe {
        description: "Minimal example plugin".into(),
        version: "0.1.0".parse().unwrap(),
        stability: Stability::Experimental,
        min_dispatcher_version: "0.1.0".parse().unwrap(),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        project_requirements: Vec::new(),
        capabilities: Vec::new(),
        command_aliases: Vec::new(),
    }
}

fn main() -> std::process::ExitCode {
    let descr = describe();
    if let Some(code) = descr.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }
    let ctx = Context::from_env();
    let mut out = Output::for_context(&ctx, std::io::stdout());
    match ctx.output_mode() {
        OutputMode::Human => out.human("hello from minimal_plugin"),
        OutputMode::Json => {
            drop(out.json(&serde_json::json!({"hello": "world"})));
        },
    }
    ExitCode::Ok.into()
}
