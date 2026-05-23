//! Rust capability provider plugin for `ready-set`.
//!
//! Contributes four capabilities (`workspace`, `toolchain`, `formatting`,
//! `linting`) to the readiness matrix and answers the `__ready` / `__set` /
//! `__go` lifecycle protocol per capability. The plugin requires a Cargo
//! workspace; outside one, lifecycle calls return
//! [`ExitCode::NotCargoWorkspace`](ready_set_sdk::ExitCode::NotCargoWorkspace).
//!
//! Contracts this provider conforms to:
//! [`capabilities.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/capabilities.md)
//! (descriptor and report shapes),
//! [`change-log.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/change-log.md)
//! (records every `__set` write for reversibility), and
//! [`ready-set-toml.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/ready-set-toml.md)
//! (the `.ready-set.toml` schema this provider seeds).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod gitignore;
pub mod manifest_edit;
pub mod members;
pub mod options;
pub mod readiness;
pub mod ready_set_toml;
pub mod runner;
pub mod templates;
pub mod workflow;
pub mod workspace;

use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::{CapabilityDescriptor, CapabilityRelevance, CapabilityVerb, ProviderId};

/// Provider id used by this plugin's capability descriptors.
pub const PROVIDER_ID: &str = "rust";

/// Return the plugin metadata payload.
#[must_use]
pub fn describe() -> Describe {
    Describe {
        description: "Rust product capabilities".into(),
        version: env!("CARGO_PKG_VERSION")
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        stability: Stability::Stable,
        min_dispatcher_version: "0.1.0"
            .parse()
            .unwrap_or_else(|_| semver::Version::new(0, 1, 0)),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        requires_cargo_workspace: true,
        capabilities: rust_capabilities(),
    }
}

/// Capability descriptors contributed by this plugin.
#[must_use]
pub fn rust_capabilities() -> Vec<CapabilityDescriptor> {
    vec![
        descriptor(
            "workspace",
            "Workspace",
            &[CapabilityVerb::Ready, CapabilityVerb::Set],
        ),
        descriptor(
            "toolchain",
            "Toolchain",
            &[CapabilityVerb::Ready, CapabilityVerb::Set],
        ),
        descriptor(
            "formatting",
            "Formatting",
            &[
                CapabilityVerb::Ready,
                CapabilityVerb::Set,
                CapabilityVerb::Go,
            ],
        ),
        descriptor(
            "linting",
            "Linting",
            &[
                CapabilityVerb::Ready,
                CapabilityVerb::Set,
                CapabilityVerb::Go,
            ],
        ),
    ]
}

fn descriptor(id: &str, title: &str, verbs: &[CapabilityVerb]) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: id.into(),
        title: title.into(),
        provider: ProviderId::from(PROVIDER_ID),
        verbs: verbs.to_vec(),
        default_relevance: CapabilityRelevance::Required,
    }
}
