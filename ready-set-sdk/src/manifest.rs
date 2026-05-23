//! Plugin manifest sidecar parsing.
//!
//! See
//! [`docs/contracts/manifest.md`](https://github.com/pulsearc-ai/ready-set/blob/main/docs/contracts/manifest.md)
//! for the source of truth.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityDescriptor;
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
    /// Whether the plugin requires a cargo workspace context.
    pub requires_cargo_workspace: bool,
    /// Product capabilities contributed by this plugin.
    pub capabilities: Vec<CapabilityDescriptor>,
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
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
requires_cargo_workspace = false
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
requires_cargo_workspace = false

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
    fn rejects_manifest_without_capabilities() {
        let toml_src = r#"
description              = "Reference plugin"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false
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
            requires_cargo_workspace: false,
            capabilities: Vec::new(),
        };
        assert!(m.validate().is_err());
        m.description = "ok".into();
        m.validate().unwrap();
    }
}
