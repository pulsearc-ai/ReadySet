//! Rotation manifest: per-secret backend + cadence configuration.
//!
//! Lives at `<project_root>/.ready-set/plugins/secrets/manifest.toml`. Created
//! and modified by `secrets set` (recorded in the SDK change log). Read by
//! `ready rotation` and `ready-set rotate`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ready_set_sdk::{Error, Result};
use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

/// Top-level default cadence applied when a secret omits `cadence_days`.
pub const DEFAULT_CADENCE_DAYS: u64 = 90;

/// Current manifest schema version.
pub const SCHEMA_VERSION: u64 = 1;

/// Parsed manifest. Lossy with respect to comments — for in-place editing
/// without losing user formatting, use [`load_document`] + `toml_edit` instead.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// `[ready-set-encrypt]` header.
    #[serde(rename = "ready-set-encrypt")]
    pub header: ManifestHeader,
    /// Per-secret entries from `[secret.<NAME>]` tables.
    #[serde(default, rename = "secret")]
    pub secrets: BTreeMap<String, SecretEntry>,
}

/// `[ready-set-encrypt]` table contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestHeader {
    /// Manifest schema version. Must equal [`SCHEMA_VERSION`].
    pub schema_version: u64,
    /// Default rotation cadence in days, used when a secret omits its own.
    #[serde(default = "default_cadence")]
    pub default_cadence_days: u64,
}

impl Default for ManifestHeader {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            default_cadence_days: DEFAULT_CADENCE_DAYS,
        }
    }
}

const fn default_cadence() -> u64 {
    DEFAULT_CADENCE_DAYS
}

/// One `[secret.<NAME>]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SecretEntry {
    /// Which backend handles rotation for this secret.
    pub backend: BackendKind,
    /// Override the manifest-level `default_cadence_days` for this secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadence_days: Option<u64>,
    /// Whether this entry should participate in rotation cadence checks.
    /// Defaults to `true`; set `false` for non-secret config values kept in
    /// the manifest only to satisfy inventory drift checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<bool>,
    /// Project-relative path the `self-issued` / `exec` backend writes the new
    /// value to. When absent, the new value is printed to stdout once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    /// URL printed by the `manual` backend reminder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dashboard_url: Option<String>,
    /// Free-form human notes (e.g. rotation caveats).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// `exec` backend: required argv array whose stdout becomes the new value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate_command: Option<Vec<String>>,
    /// Optional argv arrays run sequentially after the value is in hand.
    /// Used by both `self-issued` (after `target_path` write) and `exec` (after
    /// `generate_command`). Fail-fast: first non-zero exit halts the rest.
    /// Supports `{{value}}` and `{{value_path}}` substitution inside elements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_commands: Option<Vec<Vec<String>>>,
    /// Extra paths to add to the sandbox's allow-write list. `~` expanded.
    /// Use for provider CLIs that maintain state outside the project (e.g.
    /// `~/.fly`, `~/.config/neon`, `~/.config/gcloud`, `~/.azure`,
    /// `~/.config/op`, or `~/.kube`). Document the reason in `notes`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_write_paths: Option<Vec<String>>,
    /// When `true`, skip the macOS sandbox wrap for this secret's commands.
    /// Reserved for genuinely problematic tools; emits a runtime warning and
    /// records `sandboxed: false` in the audit log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsandboxed: Option<bool>,
    /// `webhook` backend: HTTP endpoint to POST against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    /// `webhook` backend: HTTP method. Defaults to `POST`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_method: Option<String>,
    /// `webhook` backend: request headers. Values support `{{env.NAME}}`
    /// substitution so auth tokens stay out of the committed manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_headers: Option<BTreeMap<String, String>>,
    /// `webhook` backend: request body. Supports `{{env.NAME}}` substitution.
    /// Use any content type; pair with `Content-Type` in `webhook_headers`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_body: Option<String>,
    /// `webhook` backend: dotted key path into the JSON response body. When
    /// set, the value extracted becomes the new secret value (analogous to
    /// `exec`'s `generate_command` stdout). When absent, the webhook is
    /// fire-and-forget — no value is captured, audit records
    /// `outcome: "triggered"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_response_key: Option<String>,
    /// `webhook` backend: timeout in seconds for the HTTP call. Defaults to 30.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_timeout_seconds: Option<u64>,
    /// Optional HTTP webhook deploys, run sequentially after
    /// `deploy_commands`. Each entry POSTs the freshly-rotated value to
    /// a URL with `{{value}}` substitution in body/headers. Useful when
    /// the deploy target is an HTTP API rather than a CLI. Fail-fast:
    /// first non-2xx halts the rest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_webhooks: Option<Vec<DeployWebhook>>,
}

/// One entry in `[secret.<NAME>].deploy_webhooks`.
///
/// Pushes the rotated value to an HTTP endpoint via
/// [`crate::webhook::deploy`]. Distinct from the `webhook` backend
/// itself: the backend *produces* a value from a response; a deploy
/// *consumes* an already-rotated value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployWebhook {
    /// Endpoint URL. Must be `http://` or `https://`.
    pub url: String,
    /// HTTP method. Defaults to `POST`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// Request headers. Values support `{{env.NAME}}` and `{{value}}`
    /// substitution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<BTreeMap<String, String>>,
    /// Request body. Supports `{{env.NAME}}` and `{{value}}`
    /// substitution. Pair with a `Content-Type` header.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Timeout in seconds. Defaults to 30.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u64>,
}

/// Which rotation backend handles a secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    /// 32 random bytes generated locally via `getrandom`.
    SelfIssued,
    /// User rotates manually via a dashboard URL.
    #[default]
    Manual,
    /// Value comes from the stdout of a user-defined `generate_command`.
    Exec,
    /// Value comes from (or is triggered via) an HTTP POST to a webhook.
    /// When `webhook_response_key` is set, the response body is parsed as
    /// JSON and the value is extracted from that key path; otherwise the
    /// webhook is a fire-and-forget trigger and no value is captured.
    Webhook,
}

impl BackendKind {
    /// Lowercase kebab-case label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SelfIssued => "self-issued",
            Self::Manual => "manual",
            Self::Exec => "exec",
            Self::Webhook => "webhook",
        }
    }
}

/// Project-relative path to the manifest.
#[must_use]
pub fn manifest_path(root: &Path) -> PathBuf {
    root.join(".ready-set/plugins/secrets/manifest.toml")
}

/// Load the manifest, returning `Ok(None)` when the file does not exist.
///
/// # Errors
///
/// Returns [`Error::Io`] for read failures other than `NotFound`, and
/// [`Error::TomlParse`] when the file is present but malformed or specifies
/// an unsupported `schema_version`.
pub fn load(root: &Path) -> Result<Option<Manifest>> {
    let path = manifest_path(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let manifest: Manifest =
        toml::from_str(&raw).map_err(|e| Error::TomlParse(format!("{}: {e}", path.display())))?;
    if manifest.header.schema_version != SCHEMA_VERSION {
        return Err(Error::TomlParse(format!(
            "{}: unsupported schema_version {} (expected {SCHEMA_VERSION})",
            path.display(),
            manifest.header.schema_version
        )));
    }
    validate_entries(&manifest, &path)?;
    Ok(Some(manifest))
}

fn validate_entries(manifest: &Manifest, path: &Path) -> Result<()> {
    for (name, entry) in &manifest.secrets {
        if entry.backend == BackendKind::Exec {
            let cmd = entry.generate_command.as_deref().ok_or_else(|| {
                Error::TomlParse(format!(
                    "{}: [secret.{name}] backend = \"exec\" requires `generate_command`",
                    path.display()
                ))
            })?;
            if cmd.is_empty() {
                return Err(Error::TomlParse(format!(
                    "{}: [secret.{name}] generate_command must not be empty",
                    path.display()
                )));
            }
        }
        if entry.backend == BackendKind::Webhook {
            let url = entry.webhook_url.as_deref().ok_or_else(|| {
                Error::TomlParse(format!(
                    "{}: [secret.{name}] backend = \"webhook\" requires `webhook_url`",
                    path.display()
                ))
            })?;
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::TomlParse(format!(
                    "{}: [secret.{name}] webhook_url must be http:// or https://",
                    path.display()
                )));
            }
        }
        if let Some(deploys) = entry.deploy_commands.as_deref() {
            for (i, argv) in deploys.iter().enumerate() {
                if argv.is_empty() {
                    return Err(Error::TomlParse(format!(
                        "{}: [secret.{name}] deploy_commands[{i}] must not be empty",
                        path.display()
                    )));
                }
            }
        }
        if let Some(webhooks) = entry.deploy_webhooks.as_deref() {
            for (i, webhook) in webhooks.iter().enumerate() {
                // URLs may begin with `{{env.NAME}}` so a secret-bearing
                // URL doesn't have to live in the manifest. The runtime
                // re-validates the scheme after substitution.
                let starts_with_template = webhook.url.starts_with("{{env.");
                let is_http =
                    webhook.url.starts_with("http://") || webhook.url.starts_with("https://");
                if !(is_http || starts_with_template) {
                    return Err(Error::TomlParse(format!(
                        "{}: [secret.{name}] deploy_webhooks[{i}].url must be http://, https://, \
                         or a `{{{{env.NAME}}}}` placeholder",
                        path.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Load the manifest as a `toml_edit::DocumentMut` for in-place editing
/// that preserves comments and ordering.
///
/// # Errors
///
/// Returns [`Error::Io`] on read failure and [`Error::TomlParse`] on parse failure.
pub fn load_document(root: &Path) -> Result<Option<(DocumentMut, String)>> {
    let path = manifest_path(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let doc: DocumentMut = raw
        .parse()
        .map_err(|e: toml_edit::TomlError| Error::TomlParse(format!("{}: {e}", path.display())))?;
    Ok(Some((doc, raw)))
}

/// Render a fresh manifest for first-run scaffold, seeded with the detected
/// env-var names. Every secret gets `backend = "manual"`.
#[must_use]
pub fn render_initial(detected: &BTreeSet<String>) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("# ready-set-encrypt rotation manifest. Edit per-secret `backend` and\n");
    out.push_str("# `cadence_days` here; ready/rotate consume these settings. Audit history is\n");
    out.push_str("# at .ready-set/plugins/secrets/rotations.jsonl (not in this file).\n");
    out.push('\n');
    out.push_str("[ready-set-encrypt]\n");
    let _ = writeln!(out, "schema_version = {SCHEMA_VERSION}");
    let _ = writeln!(out, "default_cadence_days = {DEFAULT_CADENCE_DAYS}");
    if !detected.is_empty() {
        out.push('\n');
        for name in detected {
            let _ = writeln!(out, "[secret.{name}]");
            out.push_str("backend = \"manual\"\n");
            out.push('\n');
        }
    }
    out
}

/// Add `[secret.<NAME>]` tables for any detected name not already present.
/// User entries and comments are preserved by `toml_edit`. Returns the set of
/// names that were newly added.
pub fn add_missing_secrets(doc: &mut DocumentMut, detected: &BTreeSet<String>) -> BTreeSet<String> {
    let existing: BTreeSet<String> = doc
        .get("secret")
        .and_then(Item::as_table)
        .map(|t| t.iter().map(|(k, _)| k.to_owned()).collect())
        .unwrap_or_default();
    let mut added = BTreeSet::new();
    for name in detected {
        if existing.contains(name) {
            continue;
        }
        let mut table = Table::new();
        table["backend"] = value(BackendKind::Manual.as_str());
        doc["secret"][name.as_str()] = Item::Table(table);
        added.insert(name.clone());
    }
    added
}

/// Remove `[secret.<NAME>]` tables outside the canonical inventory.
///
/// This is separate from additive reconcile so callers can require an explicit
/// `--force` before pruning user entries.
pub fn prune_stale_secrets(doc: &mut DocumentMut, detected: &BTreeSet<String>) -> BTreeSet<String> {
    let Some(secret_table) = doc.get_mut("secret").and_then(Item::as_table_mut) else {
        return BTreeSet::new();
    };
    let existing: Vec<String> = secret_table.iter().map(|(k, _)| k.to_owned()).collect();
    let mut removed = BTreeSet::new();
    for name in existing {
        if detected.contains(&name) {
            continue;
        }
        if secret_table.remove(&name).is_some() {
            removed.insert(name);
        }
    }
    removed
}

/// Effective cadence (days) for a given secret, falling back to the manifest's
/// `default_cadence_days`.
#[must_use]
pub fn effective_cadence(manifest: &Manifest, name: &str) -> u64 {
    manifest
        .secrets
        .get(name)
        .and_then(|s| s.cadence_days)
        .unwrap_or(manifest.header.default_cadence_days)
}

/// True when a manifest entry should participate in rotation tracking.
#[must_use]
pub const fn rotation_required(entry: &SecretEntry) -> bool {
    !matches!(entry.rotate, Some(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn render_initial_includes_header_and_secrets() {
        let rendered = render_initial(&names(&["FOO", "BAR"]));
        assert!(rendered.contains("schema_version = 1"));
        assert!(rendered.contains("default_cadence_days = 90"));
        assert!(rendered.contains("[secret.FOO]"));
        assert!(rendered.contains("[secret.BAR]"));
        assert!(rendered.contains("backend = \"manual\""));
    }

    #[test]
    fn render_initial_with_empty_detected_omits_secret_tables() {
        let rendered = render_initial(&BTreeSet::new());
        assert!(rendered.contains("[ready-set-encrypt]"));
        assert!(!rendered.contains("[secret."));
    }

    #[test]
    fn load_returns_none_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).unwrap().is_none());
    }

    #[test]
    fn load_roundtrips_render_initial() {
        let dir = tempfile::tempdir().unwrap();
        let path = manifest_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, render_initial(&names(&["FOO"]))).unwrap();
        let loaded = load(dir.path()).unwrap().unwrap();
        assert_eq!(loaded.header.schema_version, 1);
        assert_eq!(loaded.header.default_cadence_days, 90);
        let entry = loaded.secrets.get("FOO").unwrap();
        assert_eq!(entry.backend, BackendKind::Manual);
        assert!(entry.cadence_days.is_none());
    }

    #[test]
    fn load_rejects_unknown_schema_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = manifest_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "[ready-set-encrypt]\nschema_version = 999\ndefault_cadence_days = 90\n",
        )
        .unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(matches!(err, Error::TomlParse(_)));
    }

    #[test]
    fn additive_reconcile_appends_new_and_preserves_user_entries() {
        let raw = r#"# my notes
[ready-set-encrypt]
schema_version = 1
default_cadence_days = 90

[secret.FOO]
backend = "self-issued"
cadence_days = 30
notes = "do not touch"
"#;
        let mut doc: DocumentMut = raw.parse().unwrap();
        let added = add_missing_secrets(&mut doc, &names(&["FOO", "BAR"]));
        assert_eq!(added, names(&["BAR"]));
        let rendered = doc.to_string();
        assert!(rendered.contains("# my notes"));
        assert!(rendered.contains("notes = \"do not touch\""));
        assert!(rendered.contains("[secret.BAR]"));
        assert!(rendered.contains("backend = \"manual\""));
        // FOO's self-issued config survives.
        assert!(rendered.contains("backend = \"self-issued\""));
        assert!(rendered.contains("cadence_days = 30"));
    }

    #[test]
    fn prune_stale_secrets_removes_only_names_outside_inventory() {
        let raw = r#"[ready-set-encrypt]
schema_version = 1
default_cadence_days = 90

[secret.FOO]
backend = "self-issued"
cadence_days = 30
notes = "keep me"

[secret.STALE]
backend = "manual"
"#;
        let mut doc: DocumentMut = raw.parse().unwrap();

        let removed = prune_stale_secrets(&mut doc, &names(&["FOO"]));
        let rendered = doc.to_string();

        assert_eq!(removed, names(&["STALE"]));
        assert!(rendered.contains("[secret.FOO]"));
        assert!(rendered.contains("notes = \"keep me\""));
        assert!(!rendered.contains("[secret.STALE]"));
    }

    #[test]
    fn effective_cadence_falls_back_to_default() {
        let mut manifest = Manifest::default();
        manifest.header.default_cadence_days = 60;
        manifest.secrets.insert(
            "PINNED".into(),
            SecretEntry {
                backend: BackendKind::Manual,
                cadence_days: Some(7),
                ..SecretEntry::default()
            },
        );
        manifest
            .secrets
            .insert("DEFAULTED".into(), SecretEntry::default());
        assert_eq!(effective_cadence(&manifest, "PINNED"), 7);
        assert_eq!(effective_cadence(&manifest, "DEFAULTED"), 60);
        assert_eq!(effective_cadence(&manifest, "UNKNOWN"), 60);
    }
}
