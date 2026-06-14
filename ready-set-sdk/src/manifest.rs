//! Plugin manifest sidecar parsing.
//!
//! See `docs/contracts/manifest.md` for the source of truth.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityDescriptor;
use crate::command_alias::CommandAlias;
use crate::describe::{Platform, Stability};
use crate::error::{Error, Result};

/// Parsed `ready-set-<name>.toml` sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// One-line summary, max 80 chars.
    pub description: String,
    /// Plugin semver.
    pub version: semver::Version,
    /// Stability tier.
    pub stability: Stability,
    /// Minimum dispatcher semver this plugin requires.
    pub min_dispatcher_version: semver::Version,
    /// Supported operating systems.
    pub platforms: Vec<Platform>,
    /// Plugin-declared project requirements, such as `cargo-workspace`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub project_requirements: Vec<String>,
    /// Product capabilities contributed by this plugin.
    pub capabilities: Vec<CapabilityDescriptor>,
    /// User-facing `ready-set <name>` aliases contributed by this plugin.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub command_aliases: Vec<CommandAlias>,
}

impl Manifest {
    /// Load and validate a manifest sidecar from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the file cannot be read,
    /// [`Error::TomlParse`] if the file is not valid TOML or does not match
    /// the schema, or [`Error::ContractViolation`] if the schema is satisfied
    /// but the values violate manifest rules (e.g. description too long).
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let manifest: Self = toml::from_str(&raw)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Compute the expected sidecar path for a binary at `binary`.
    #[must_use]
    pub fn sibling_of(binary: &Path) -> PathBuf {
        let mut path = binary.to_path_buf();
        // Strip any extension (e.g. `.exe` on Windows) before appending `.toml`.
        path.set_extension("toml");
        path
    }

    fn validate(&self) -> Result<()> {
        if self.description.is_empty() {
            return Err(Error::contract("manifest description is empty"));
        }
        if self.description.len() > 80 {
            return Err(Error::contract(format!(
                "manifest description is {} chars (max 80)",
                self.description.len()
            )));
        }
        if self.description.contains('\n') || self.description.contains('\r') {
            return Err(Error::contract(
                "manifest description must be a single line",
            ));
        }
        if self.platforms.is_empty() {
            return Err(Error::contract(
                "manifest platforms must list at least one OS",
            ));
        }
        for requirement in &self.project_requirements {
            if requirement.is_empty() {
                return Err(Error::contract("manifest project requirement is empty"));
            }
            if requirement.contains(char::is_whitespace) {
                return Err(Error::contract(format!(
                    "manifest project requirement `{requirement}` contains whitespace"
                )));
            }
        }
        for alias in &self.command_aliases {
            if alias.name.is_empty() {
                return Err(Error::contract("manifest command alias name is empty"));
            }
            if alias.description.is_empty() {
                return Err(Error::contract(format!(
                    "manifest command alias `{}` description is empty",
                    alias.name
                )));
            }
            if alias.description.len() > 80 {
                return Err(Error::contract(format!(
                    "manifest command alias `{}` description is {} chars (max 80)",
                    alias.name,
                    alias.description.len()
                )));
            }
            if alias.description.contains('\n') || alias.description.contains('\r') {
                return Err(Error::contract(format!(
                    "manifest command alias `{}` description must be a single line",
                    alias.name
                )));
            }
            if alias.name.contains(char::is_whitespace) {
                return Err(Error::contract(format!(
                    "manifest command alias `{}` contains whitespace",
                    alias.name
                )));
            }
            if alias.match_first_arg.as_ref().is_some_and(String::is_empty) {
                return Err(Error::contract(format!(
                    "manifest command alias `{}` match_first_arg is empty",
                    alias.name
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command_alias::CommandAliasTarget;

    #[test]
    fn sibling_of_strips_extension() {
        let unix = Path::new("/usr/local/bin/ready-set-foo");
        assert_eq!(
            Manifest::sibling_of(unix),
            PathBuf::from("/usr/local/bin/ready-set-foo.toml")
        );

        let win = Path::new("C:/tools/ready-set-foo.exe");
        assert_eq!(
            Manifest::sibling_of(win),
            PathBuf::from("C:/tools/ready-set-foo.toml")
        );
    }

    #[test]
    fn parses_a_full_manifest() {
        let toml_src = r#"
description              = "Reference plugin"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
capabilities             = []
"#;
        let m: Manifest = toml::from_str(toml_src).unwrap();
        m.validate().unwrap();
        assert_eq!(m.description, "Reference plugin");
        assert_eq!(m.platforms.len(), 3);
        assert!(m.capabilities.is_empty());
    }

    #[test]
    fn parses_manifest_with_capability() {
        let toml_src = r#"
description              = "Reference plugin"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]

[[capabilities]]
id = "linting"
title = "Linting"
provider = "rust"
verbs = ["ready", "set", "go"]
default_relevance = "required"
"#;
        let m: Manifest = toml::from_str(toml_src).unwrap();
        m.validate().unwrap();
        assert_eq!(m.capabilities.len(), 1);
        assert_eq!(m.capabilities[0].id.as_str(), "linting");
    }

    #[test]
    fn parses_manifest_with_command_aliases() {
        let toml_src = r#"
description              = "Reference plugin"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
capabilities             = []

[[command_aliases]]
name = "rotate"
description = "Rotate due secrets"
target = "go"
capability = "rotation"

[[command_aliases]]
name = "encrypt"
description = "Inspect secret bundles"
match_first_arg = "bundle"
target = "plugin"
"#;
        let m: Manifest = toml::from_str(toml_src).unwrap();
        m.validate().unwrap();
        assert_eq!(m.command_aliases.len(), 2);
        assert!(matches!(
            m.command_aliases[0].target,
            CommandAliasTarget::Go { .. }
        ));
        assert_eq!(
            m.command_aliases[1].match_first_arg.as_deref(),
            Some("bundle")
        );
    }

    #[test]
    fn rejects_manifest_without_capabilities() {
        let toml_src = r#"
description              = "Reference plugin"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
"#;
        assert!(toml::from_str::<Manifest>(toml_src).is_err());
    }

    #[test]
    fn rejects_overlong_description() {
        let mut m = Manifest {
            description: "x".repeat(81),
            version: semver::Version::new(0, 1, 0),
            stability: Stability::Stable,
            min_dispatcher_version: semver::Version::new(0, 1, 0),
            platforms: vec![Platform::Linux],
            project_requirements: Vec::new(),
            capabilities: Vec::new(),
            command_aliases: Vec::new(),
        };
        assert!(m.validate().is_err());
        m.description = "ok".into();
        m.validate().unwrap();
    }
}
