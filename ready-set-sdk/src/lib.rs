//! # ready-set-sdk
//!
//! Shared conventions and helpers for `ready-set` plugins.
//!
//! The SDK is the typed Rust mirror of the contracts under
//! [`docs/contracts/`](https://github.com/pulsearc-ai/ReadySet/tree/main/docs/contracts).
//! Plugins are not required to use the SDK — any binary on PATH can be a
//! plugin — but using it makes plugins consistent with first-party tools and
//! saves writing the same boilerplate.
//!
//! ## What lives here
//!
//! - [`context`]: per-invocation state ([`Context`]), populated from the
//!   `READY_SET_*` env contract.
//! - [`capability`]: product capability descriptors and lifecycle reports.
//! - [`output`]: human/JSON output formatting ([`Output`]).
//! - [`exit_code`]: documented process exit codes ([`ExitCode`]).
//! - [`change_log`]: append-only JSONL change log for reversibility.
//! - [`describe`]: `__describe` subcommand support.
//! - [`manifest`]: plugin manifest sidecar parsing.
//! - [`sandbox`]: per-platform sandbox trait (no-op stubs in v0.1.0).
//! - [`config`]: `.ready-set.toml` loader.
//! - [`fs`]: filesystem helpers (atomic writes, hashing).
//! - [`dispatch`]: cross-plugin dispatch helper.
//! - [`logging`]: tracing setup honoring the env contract.
//! - [`error`]: SDK-wide error type.
//!
//! ## Minimal plugin shape
//!
//! ```no_run
//! use ready_set_sdk::prelude::*;
//! use ready_set_sdk::describe::{Describe, Stability, Platform};
//!
//! fn describe() -> Describe {
//!     Describe {
//!         description: "Example plugin".into(),
//!         version: "0.1.0".parse().unwrap(),
//!         stability: Stability::Experimental,
//!         min_dispatcher_version: "0.1.0".parse().unwrap(),
//!         platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
//!         requires_cargo_workspace: false,
//!         capabilities: Vec::new(),
//!     }
//! }
//!
//! fn main() -> std::process::ExitCode {
//!     let descr = describe();
//!     if let Some(code) = descr.handle_arg0_describe(std::env::args_os()) {
//!         return code.into();
//!     }
//!     let ctx = Context::from_env();
//!     let mut out = Output::for_context(&ctx, std::io::stdout());
//!     out.human("hello world");
//!     ExitCode::Ok.into()
//! }
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capability;
pub mod change_log;
pub mod config;
pub mod context;
pub mod describe;
pub mod dispatch;
pub mod error;
pub mod exit_code;
pub mod fs;
pub mod lifecycle;
pub mod logging;
pub mod manifest;
pub mod output;
pub mod prelude;
pub mod sandbox;

pub use capability::{
    CapabilityAction, CapabilityActionKind, CapabilityDescriptor, CapabilityId,
    CapabilityRelevance, CapabilityReport, CapabilityRunReport, CapabilityState, CapabilityVerb,
    NextAction, ProviderId, RunStatus,
};
pub use context::Context;
pub use error::{Error, Result};
pub use exit_code::ExitCode;
pub use lifecycle::{LifecycleRequest, LifecycleRequestError, parse_lifecycle_request};
pub use output::{Output, OutputMode};
