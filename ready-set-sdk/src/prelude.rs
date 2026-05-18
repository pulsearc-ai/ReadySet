//! Convenient re-exports for plugin authors.
//!
//! ```no_run
//! use ready_set_sdk::prelude::*;
//!
//! let ctx = Context::from_env();
//! let mut out = Output::for_context(&ctx, std::io::stdout());
//! out.human("hello");
//! # let _ = ExitCode::Ok;
//! ```

pub use crate::capability::{
    CapabilityAction, CapabilityActionKind, CapabilityDescriptor, CapabilityId,
    CapabilityRelevance, CapabilityReport, CapabilityRunReport, CapabilityState, CapabilityVerb,
    NextAction, ProviderId, RunStatus,
};
pub use crate::context::{ColorMode, Context, LogLevel};
pub use crate::error::{Error, Result};
pub use crate::exit_code::ExitCode;
pub use crate::lifecycle::{LifecycleRequest, LifecycleRequestError, parse_lifecycle_request};
pub use crate::output::{Output, OutputMode};
