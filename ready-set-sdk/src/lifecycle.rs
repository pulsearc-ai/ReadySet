//! Lifecycle protocol helpers for provider plugins.

use std::ffi::OsString;

use crate::CapabilityId;

/// Lifecycle protocol request parsed from plugin argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleRequest {
    /// Read-only readiness report request.
    Ready {
        /// Capability id to diagnose.
        capability: CapabilityId,
    },
    /// Setup/reconciliation request.
    Set {
        /// Capability id to reconcile.
        capability: CapabilityId,
        /// Remaining provider-specific arguments.
        args: Vec<OsString>,
    },
    /// Workflow execution request.
    Go {
        /// Capability id to execute.
        capability: CapabilityId,
        /// Remaining provider-specific arguments.
        args: Vec<OsString>,
    },
}

/// Error produced when a lifecycle protocol request is malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleRequestError {
    message: String,
}

impl LifecycleRequestError {
    /// Human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for LifecycleRequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LifecycleRequestError {}

/// Parse `__ready`, `__set`, or `__go` from a plugin argv iterator.
///
/// Returns `Ok(None)` when the first non-program argument is not a lifecycle
/// protocol command, so the plugin can continue with normal CLI parsing.
///
/// # Errors
///
/// Returns [`LifecycleRequestError`] when a lifecycle protocol command is
/// present but missing its required capability argument or, for `__ready`,
/// includes extra arguments.
pub fn parse_lifecycle_request(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<LifecycleRequest>, LifecycleRequestError> {
    let mut args = args.into_iter();
    drop(args.next());
    let Some(command) = args.next() else {
        return Ok(None);
    };
    let Some(command) = command.to_str() else {
        return Ok(None);
    };

    match command {
        "__ready" => {
            let capability = required_capability(command, args.next())?;
            if args.next().is_some() {
                return Err(error("__ready accepts exactly one capability argument"));
            }
            Ok(Some(LifecycleRequest::Ready { capability }))
        },
        "__set" => {
            let capability = required_capability(command, args.next())?;
            Ok(Some(LifecycleRequest::Set {
                capability,
                args: args.collect(),
            }))
        },
        "__go" => {
            let capability = required_capability(command, args.next())?;
            Ok(Some(LifecycleRequest::Go {
                capability,
                args: args.collect(),
            }))
        },
        _ => Ok(None),
    }
}

fn required_capability(
    command: &str,
    raw: Option<OsString>,
) -> Result<CapabilityId, LifecycleRequestError> {
    let Some(raw) = raw else {
        return Err(error(format!("{command} requires a capability argument")));
    };
    let Some(raw) = raw.to_str() else {
        return Err(error(format!(
            "{command} capability argument must be valid UTF-8"
        )));
    };
    if raw.is_empty() {
        return Err(error(format!(
            "{command} capability argument must not be empty"
        )));
    }
    Ok(CapabilityId::from(raw))
}

fn error(message: impl Into<String>) -> LifecycleRequestError {
    LifecycleRequestError {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn ignores_non_lifecycle_commands() {
        assert_eq!(
            parse_lifecycle_request(os(&["plugin", "run"])).unwrap(),
            None
        );
    }

    #[test]
    fn parses_ready_request() {
        let request = parse_lifecycle_request(os(&["plugin", "__ready", "linting"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            request,
            LifecycleRequest::Ready {
                capability: "linting".into()
            }
        );
    }

    #[test]
    fn parses_set_request_with_passthrough_args() {
        let request = parse_lifecycle_request(os(&[
            "plugin",
            "__set",
            "formatting",
            "--dry-run",
            "--force",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            request,
            LifecycleRequest::Set {
                capability: "formatting".into(),
                args: os(&["--dry-run", "--force"]),
            }
        );
    }

    #[test]
    fn rejects_ready_extra_args() {
        let err =
            parse_lifecycle_request(os(&["plugin", "__ready", "linting", "extra"])).unwrap_err();
        assert!(err.to_string().contains("exactly one"));
    }
}
