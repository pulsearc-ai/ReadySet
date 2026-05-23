//! `.ready-set.toml` loading.
//!
//! See
//! [`docs/contracts/ready-set-toml.md`](https://github.com/pulsearc-ai/ready-set/blob/main/docs/contracts/ready-set-toml.md)
//! for the source of truth.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::capability::{CapabilityRelevance, ProviderId};
use crate::error::{Error, Result};

/// Project metadata under the `[ready-set]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ProjectMeta {
    /// Schema version for this file. Always `2` for the capability lifecycle.
    pub schema_version: u32,
    /// Product profile used to interpret capability relevance.
    pub profile: String,
}

/// Per-capability configuration under `[capabilities.<id>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityConfig {
    /// Effective relevance override for this product.
    pub relevance: Option<CapabilityRelevance>,
    /// Provider override for this capability.
    pub provider: Option<ProviderId>,
    /// Keys in this capability table not understood by this SDK version.
    pub unknown_keys: Vec<String>,
}

/// Loaded `.ready-set.toml`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path the config was loaded from.
    pub path: PathBuf,
    /// `[ready-set]` table.
    pub ready_set: ProjectMeta,
    /// Per-capability configuration, keyed by capability id.
    pub capabilities: BTreeMap<String, CapabilityConfig>,
    /// Per-plugin sections, keyed by plugin name.
    ///
    /// The `toml::Value` exposure here leaks `toml = 0.8.x` semver into
    /// the SDK's public API. Before `0.1.0` stable this will be wrapped
    /// in an opaque `PluginSection` type so the underlying value
    /// representation becomes a private implementation detail.
    pub plugins: BTreeMap<String, toml::Value>,
    /// Sections under `[ready-set]` not understood by this SDK version.
    /// Surfaces forward-compat warnings without crashing on extras.
    pub unknown_keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    #[serde(rename = "ready-set")]
    ready_set: toml::Value,
    #[serde(default)]
    capabilities: BTreeMap<String, RawCapabilityConfig>,
    #[serde(flatten)]
    rest: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Deserialize)]
struct RawCapabilityConfig {
    #[serde(default)]
    relevance: Option<CapabilityRelevance>,
    #[serde(default)]
    provider: Option<ProviderId>,
    #[serde(flatten)]
    rest: BTreeMap<String, toml::Value>,
}

/// Walk upward from `start` looking for `.ready-set.toml`. Returns
/// `Ok(None)` if no file is found before the filesystem root.
///
/// # Errors
///
/// Returns [`Error::Io`] if a directory cannot be read or the config file
/// cannot be opened, [`Error::TomlParse`] if the file is not valid TOML, or
/// [`Error::ContractViolation`] if the file is structurally valid TOML but
/// missing required keys.
pub fn load_config(start: &Path) -> Result<Option<Config>> {
    let Some(found) = find_upwards(start) else {
        return Ok(None);
    };
    Ok(Some(parse_at(&found)?))
}

/// Parse the config at `path` directly, without walking.
///
/// # Errors
///
/// Same conditions as [`load_config`].
pub fn parse_at(path: &Path) -> Result<Config> {
    let raw = std::fs::read_to_string(path)?;
    let parsed: RawConfig = toml::from_str(&raw)?;

    if let toml::Value::Table(t) = &parsed.ready_set
        && let Some(version) = t.get("schema_version").and_then(toml::Value::as_integer)
        && version != 2
    {
        return Err(Error::contract(format!(
            ".ready-set.toml schema_version {version} is unsupported; expected 2"
        )));
    }

    let ready_set: ProjectMeta =
        parsed
            .ready_set
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| {
                Error::contract(format!("[ready-set] missing required keys: {e}"))
            })?;
    if ready_set.schema_version != 2 {
        return Err(Error::contract(format!(
            ".ready-set.toml schema_version {} is unsupported; expected 2",
            ready_set.schema_version
        )));
    }

    let mut unknown_keys = Vec::new();
    if let toml::Value::Table(t) = &parsed.ready_set {
        for k in t.keys() {
            if !matches!(k.as_str(), "schema_version" | "profile") {
                unknown_keys.push(k.clone());
            }
        }
    }

    let capabilities = parsed
        .capabilities
        .into_iter()
        .map(|(id, raw)| {
            let cfg = CapabilityConfig {
                relevance: raw.relevance,
                provider: raw.provider,
                unknown_keys: raw.rest.into_keys().collect(),
            };
            (id, cfg)
        })
        .collect();

    Ok(Config {
        path: path.to_path_buf(),
        ready_set,
        capabilities,
        plugins: parsed.rest,
        unknown_keys,
    })
}

fn find_upwards(start: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join(".ready-set.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        cur = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(
            &path,
            "[ready-set]\nschema_version = 2\nprofile = \"rust-workspace\"\n",
        )
        .unwrap();

        let cfg = parse_at(&path).unwrap();
        assert_eq!(cfg.ready_set.schema_version, 2);
        assert_eq!(cfg.ready_set.profile, "rust-workspace");
        assert!(cfg.capabilities.is_empty());
        assert!(cfg.plugins.is_empty());
        assert!(cfg.unknown_keys.is_empty());
    }

    #[test]
    fn rejects_v1_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(&path, "[ready-set]\nschema_version = 1\n").unwrap();

        let err = parse_at(&path).unwrap_err();
        assert!(err.to_string().contains("schema_version 1 is unsupported"));
    }

    #[test]
    fn rejects_missing_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(&path, "[ready-set]\nschema_version = 2\n").unwrap();

        let err = parse_at(&path).unwrap_err();
        assert!(err.to_string().contains("missing required keys"));
    }

    #[test]
    fn parses_v2_capability_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(
            &path,
            r#"
[ready-set]
schema_version = 2
profile = "rust-workspace"

[capabilities.workspace]
relevance = "required"
provider = "rust"

[capabilities.toolchain]
relevance = "required"
provider = "rust"

[capabilities.formatting]
relevance = "required"
provider = "rust"

[capabilities.linting]
relevance = "optional"
provider = "rust"
"#,
        )
        .unwrap();

        let cfg = parse_at(&path).unwrap();
        assert_eq!(cfg.capabilities.len(), 4);
        assert_eq!(
            cfg.capabilities["linting"].relevance,
            Some(CapabilityRelevance::Optional)
        );
        assert_eq!(
            cfg.capabilities["workspace"]
                .provider
                .as_ref()
                .map(ProviderId::as_str),
            Some("rust")
        );
    }

    #[test]
    fn captures_per_plugin_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(
            &path,
            "[ready-set]\nschema_version = 2\nprofile = \"rust-workspace\"\n[scan]\nexclude = [\"vendor/**\"]\n",
        )
        .unwrap();

        let cfg = parse_at(&path).unwrap();
        assert!(cfg.plugins.contains_key("scan"));
        assert!(!cfg.plugins.contains_key("capabilities"));
    }

    #[test]
    fn collects_unknown_ready_set_keys_as_warnings_not_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(
            &path,
            "[ready-set]\nschema_version = 2\nprofile = \"rust-workspace\"\nfuture_field = \"hi\"\n",
        )
        .unwrap();

        let cfg = parse_at(&path).unwrap();
        assert!(cfg.unknown_keys.contains(&"future_field".to_string()));
    }

    #[test]
    fn collects_unknown_capability_keys_as_warnings_not_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".ready-set.toml");
        std::fs::write(
            &path,
            r#"
[ready-set]
schema_version = 2
profile = "rust-workspace"

[capabilities.custom]
relevance = "not-needed"
provider = "custom"
future_field = "hi"
"#,
        )
        .unwrap();

        let cfg = parse_at(&path).unwrap();
        assert_eq!(
            cfg.capabilities["custom"].relevance,
            Some(CapabilityRelevance::NotNeeded)
        );
        assert!(
            cfg.capabilities["custom"]
                .unknown_keys
                .contains(&"future_field".to_string())
        );
    }

    #[test]
    fn walks_upward() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("a/b/c");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            dir.path().join(".ready-set.toml"),
            "[ready-set]\nschema_version = 2\nprofile = \"rust-workspace\"\n",
        )
        .unwrap();
        let cfg = load_config(&inner).unwrap().unwrap();
        assert_eq!(cfg.ready_set.schema_version, 2);
    }
}
