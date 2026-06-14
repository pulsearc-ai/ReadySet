//! Provider-declared user-facing command aliases.

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityId;

/// A user-facing `ready-set <name>` command contributed by a plugin.
///
/// Aliases let providers expose short commands without hardcoding
/// provider-specific knowledge in the dispatcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandAlias {
    /// Command name after `ready-set`.
    pub name: String,
    /// One-line description shown by `ready-set --list`.
    pub description: String,
    /// Optional first argument required for this alias to match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_first_arg: Option<String>,
    /// What the dispatcher should run when this alias matches.
    #[serde(flatten)]
    pub target: CommandAliasTarget,
}

/// Target for a provider-declared command alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "target", rename_all = "kebab-case")]
pub enum CommandAliasTarget {
    /// Invoke `<provider> __set <capability> ...`.
    Set {
        /// Capability id passed to the provider lifecycle command.
        capability: CapabilityId,
    },
    /// Invoke `<provider> __go <capability> ...`.
    Go {
        /// Capability id passed to the provider lifecycle command.
        capability: CapabilityId,
    },
    /// Invoke the plugin binary directly with optional prefixed arguments.
    Plugin {
        /// Arguments prepended before the user-provided alias arguments.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        args: Vec<String>,
    },
}

impl CommandAlias {
    /// Return true when this alias should handle `user_args`.
    #[must_use]
    pub fn matches_args(&self, user_args: &[std::ffi::OsString]) -> bool {
        self.match_first_arg.as_ref().is_none_or(|expected| {
            user_args
                .first()
                .and_then(|arg| arg.to_str())
                .is_some_and(|actual| actual == expected)
        })
    }

    /// Aliases with an argument predicate are more specific.
    #[must_use]
    pub const fn specificity(&self) -> u8 {
        if self.match_first_arg.is_some() { 1 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    #[test]
    fn matches_optional_first_arg() {
        let default = CommandAlias {
            name: "encrypt".into(),
            description: "encrypt".into(),
            match_first_arg: None,
            target: CommandAliasTarget::Set {
                capability: "secret-bundles".into(),
            },
        };
        let specific = CommandAlias {
            name: "encrypt".into(),
            description: "bundle".into(),
            match_first_arg: Some("bundle".into()),
            target: CommandAliasTarget::Plugin { args: Vec::new() },
        };

        assert!(default.matches_args(&[]));
        assert!(specific.matches_args(&[OsString::from("bundle")]));
        assert!(!specific.matches_args(&[OsString::from("--dry-run")]));
        assert!(specific.specificity() > default.specificity());
    }
}
