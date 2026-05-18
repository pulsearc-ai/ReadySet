//! Capability lifecycle contract types.
//!
//! These are the typed Rust mirrors of `docs/contracts/capabilities.md`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Stable capability identifier, e.g. `linting` or `release`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    /// Create a capability id.
    ///
    /// Validation is currently handled by the JSON schema contract; this
    /// constructor stores the string as provided.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for CapabilityId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for CapabilityId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CapabilityId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Stable provider identifier, usually a plugin subcommand such as `rust`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProviderId(String);

impl ProviderId {
    /// Create a provider id.
    ///
    /// Validation is currently handled by the JSON schema contract; this
    /// constructor stores the string as provided.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the id as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for ProviderId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ProviderId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ProviderId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// Lifecycle verb supported by a capability descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityVerb {
    /// Read-only readiness diagnosis.
    Ready,
    /// Configure or reconcile the capability.
    Set,
    /// Execute the capability's main workflow.
    Go,
}

/// Readiness state for a capability report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityState {
    /// Present and usable.
    Ready,
    /// No implementation or required artifact exists.
    Missing,
    /// Present, but not fully configured.
    Incomplete,
    /// Cannot be evaluated or run until a dependency is resolved.
    Blocked,
    /// Exists, but no longer matches the declared product state.
    Stale,
    /// Available but not required for this product.
    Optional,
    /// Explicitly irrelevant for this product.
    NotNeeded,
}

/// Product relevance for a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityRelevance {
    /// Required for this product.
    Required,
    /// Available but not required.
    Optional,
    /// Explicitly irrelevant for this product.
    NotNeeded,
}

/// Static metadata for one product capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    /// Stable capability id.
    pub id: CapabilityId,
    /// Human label for matrix and help output.
    pub title: String,
    /// Stable provider id.
    pub provider: ProviderId,
    /// Supported lifecycle verbs.
    pub verbs: Vec<CapabilityVerb>,
    /// Default product relevance.
    pub default_relevance: CapabilityRelevance,
}

/// Suggested next command for a capability report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextAction {
    /// Command the user can run.
    pub command: String,
    /// Human description of what the command does.
    pub description: String,
}

/// Read-only readiness status for one product capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// Stable capability id.
    pub id: CapabilityId,
    /// Human label for matrix and help output.
    pub title: String,
    /// Stable provider id.
    pub provider: ProviderId,
    /// Current readiness state.
    pub state: CapabilityState,
    /// Effective product relevance.
    pub relevance: CapabilityRelevance,
    /// Short explanation of the current state.
    pub summary: String,
    /// Suggested next command, or `None` when no action is needed.
    pub next_action: Option<NextAction>,
}

/// Status of a lifecycle run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    /// The run completed successfully without a more specific status.
    Ok,
    /// The run changed project state.
    Changed,
    /// The run found no work to do.
    Noop,
    /// The run failed.
    Failed,
}

/// Kind of action included in a lifecycle run report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityActionKind {
    /// Created a filesystem path.
    Create,
    /// Modified a filesystem path.
    Modify,
    /// Deleted a filesystem path.
    Delete,
    /// Ran a command.
    Run,
    /// Checked state without modifying it.
    Check,
    /// Skipped an action.
    Skip,
    /// Encountered an error.
    Error,
}

/// One action checked, skipped, executed, or failed during a lifecycle run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityAction {
    /// Action kind.
    pub kind: CapabilityActionKind,
    /// Short action summary.
    pub summary: String,
    /// Project-relative path, when the action concerns a filesystem path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Structured result from running `set` or `go` for a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRunReport {
    /// Stable capability id.
    pub id: CapabilityId,
    /// Lifecycle verb that ran. Only `set` and `go` are valid on the wire.
    #[serde(
        serialize_with = "run_verb::serialize",
        deserialize_with = "run_verb::deserialize"
    )]
    pub verb: CapabilityVerb,
    /// Overall run status.
    pub status: RunStatus,
    /// Ordered actions checked, skipped, executed, or failed.
    pub actions: Vec<CapabilityAction>,
}

mod run_verb {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::CapabilityVerb;

    #[allow(clippy::trivially_copy_pass_by_ref)] // serde `serialize_with` requires &T
    pub(super) fn serialize<S>(verb: &CapabilityVerb, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::Error as _;

        match verb {
            CapabilityVerb::Set => serializer.serialize_str("set"),
            CapabilityVerb::Go => serializer.serialize_str("go"),
            CapabilityVerb::Ready => Err(S::Error::custom("ready is not valid in a run report")),
        }
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<CapabilityVerb, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error as _;

        let raw = String::deserialize(deserializer)?;
        match raw.as_str() {
            "set" => Ok(CapabilityVerb::Set),
            "go" => Ok(CapabilityVerb::Go),
            other => Err(D::Error::unknown_variant(other, &["set", "go"])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_round_trips_json() {
        let descriptor = CapabilityDescriptor {
            id: "linting".into(),
            title: "Linting".into(),
            provider: "rust".into(),
            verbs: vec![
                CapabilityVerb::Ready,
                CapabilityVerb::Set,
                CapabilityVerb::Go,
            ],
            default_relevance: CapabilityRelevance::Required,
        };

        let json = serde_json::to_string(&descriptor).unwrap();
        let back: CapabilityDescriptor = serde_json::from_str(&json).unwrap();
        assert_eq!(descriptor, back);
        assert!(json.contains("\"default_relevance\":\"required\""));
    }

    #[test]
    fn report_round_trips_json_with_null_next_action() {
        let report = CapabilityReport {
            id: "deploy".into(),
            title: "Deploy".into(),
            provider: "deploy".into(),
            state: CapabilityState::NotNeeded,
            relevance: CapabilityRelevance::NotNeeded,
            summary: "deployment is not needed".into(),
            next_action: None,
        };

        let json = serde_json::to_string(&report).unwrap();
        let back: CapabilityReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
        assert!(json.contains("\"state\":\"not-needed\""));
        assert!(json.contains("\"next_action\":null"));
    }

    #[test]
    fn run_report_round_trips_json() {
        let report = CapabilityRunReport {
            id: "linting".into(),
            verb: CapabilityVerb::Set,
            status: RunStatus::Changed,
            actions: vec![
                CapabilityAction {
                    kind: CapabilityActionKind::Create,
                    summary: "created clippy config".into(),
                    path: Some("clippy.toml".into()),
                },
                CapabilityAction {
                    kind: CapabilityActionKind::Run,
                    summary: "ran clippy".into(),
                    path: None,
                },
                CapabilityAction {
                    kind: CapabilityActionKind::Error,
                    summary: "clippy failed".into(),
                    path: None,
                },
            ],
        };

        let json = serde_json::to_string(&report).unwrap();
        let back: CapabilityRunReport = serde_json::from_str(&json).unwrap();
        assert_eq!(report, back);
    }

    #[test]
    fn run_report_rejects_ready_verb_on_wire() {
        let report = CapabilityRunReport {
            id: "linting".into(),
            verb: CapabilityVerb::Ready,
            status: RunStatus::Ok,
            actions: Vec::new(),
        };
        assert!(serde_json::to_string(&report).is_err());

        let err = serde_json::from_str::<CapabilityRunReport>(
            r#"{"id":"linting","verb":"ready","status":"ok","actions":[]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown variant"));
    }
}
