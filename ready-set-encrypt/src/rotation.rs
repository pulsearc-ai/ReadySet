//! Backend dispatch + audit log for the `rotation` capability.
//!
//! Audit log lives at `<project_root>/.ready-set/plugins/secrets/rotations.jsonl`
//! and is **append-only**. It is intentionally not routed through the SDK
//! change log: `ready-set undo` should not "restore" stale `last_rotated`
//! timestamps after the upstream secret has actually rotated.

use std::path::{Path, PathBuf};

use ready_set_sdk::fs::{atomic_write, restrict_to_user, sha256_bytes};
use ready_set_sdk::{Error, Result};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use time::OffsetDateTime;

use crate::exec::{self, CommandOutput};
use crate::manifest::{
    BackendKind, DeployWebhook, Manifest, SecretEntry, effective_cadence, rotation_required,
};

/// Project-relative path to the audit log.
#[must_use]
pub fn audit_log_path(root: &Path) -> PathBuf {
    root.join(".ready-set/plugins/secrets/rotations.jsonl")
}

/// One audit-log entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotationEvent {
    /// Secret name (env var key).
    pub name: String,
    /// Backend kind label (e.g. `"self-issued"`, `"manual"`, `"exec"`).
    pub backend: String,
    /// Event timestamp (RFC3339 UTC on the wire).
    #[serde(with = "rfc3339_utc")]
    pub ts: OffsetDateTime,
    /// Result of the rotation attempt.
    pub outcome: Outcome,
    /// SHA-256 of the new value, populated only for successful `Rotated`
    /// outcomes that produced one. Never the raw value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_sha256: Option<String>,
    /// Manifest-declared `target_path`, when one was configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_path: Option<String>,
    /// Error string when `outcome` is `Failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Count of deploy commands that ran successfully. `None` when the
    /// rotation predates v0.3 or had no deploy commands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_count: Option<u32>,
    /// Count of deploy webhooks that ran successfully (2xx response).
    /// `None` when the entry had no `deploy_webhooks`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deploy_webhook_count: Option<u32>,
    /// Was the wrap actually applied to the spawned commands? `None` for
    /// pre-v0.3 entries and for `manual` outcomes (no commands run).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandboxed: Option<bool>,
    /// Stable label for the sandbox backend, when one was used (e.g.
    /// `"macos-sandbox-exec"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform_sandbox: Option<String>,
}

/// Per-event outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// Secret was rotated successfully.
    Rotated,
    /// User was reminded; manual rotation required upstream.
    Reminded,
    /// Webhook fired successfully but produced no value (fire-and-forget).
    /// Upstream rotation may or may not have happened — we only know we
    /// pushed the trigger.
    Triggered,
    /// Backend dispatch failed.
    Failed,
}

impl RotationEvent {
    /// Human-readable label for this event's outcome.
    #[must_use]
    pub const fn outcome_label(&self) -> &'static str {
        match self.outcome {
            Outcome::Rotated => "rotated",
            Outcome::Reminded => "reminded",
            Outcome::Triggered => "triggered",
            Outcome::Failed => "failed",
        }
    }
}

/// Cadence classification for one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cadence {
    /// Entry is tracked in the manifest but excluded from rotation cadence.
    NotNeeded,
    /// Never rotated. Treated as due now.
    NeverRotated,
    /// Within cadence — no action required.
    Healthy {
        /// Days remaining before rotation is due.
        days_until_due: i64,
    },
    /// Overdue — rotation recommended.
    Overdue {
        /// Days past the rotation due date.
        days_overdue: i64,
    },
}

impl Cadence {
    /// True when the secret should be rotated on the next confirmed rotation.
    #[must_use]
    pub const fn needs_rotation(self) -> bool {
        matches!(self, Self::NeverRotated | Self::Overdue { .. })
    }
}

/// Read all audit events. Malformed lines are skipped silently to keep the
/// readiness check resilient.
///
/// # Errors
///
/// Returns [`Error::Io`] when the file exists but cannot be read.
pub fn read_events(root: &Path) -> Result<Vec<RotationEvent>> {
    let path = audit_log_path(root);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(Error::Io(err)),
    };
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<RotationEvent>(trimmed) {
            out.push(event);
        }
    }
    Ok(out)
}

/// Append one event to the audit log. Creates parent directories if needed.
///
/// # Errors
///
/// Returns [`Error::Io`] for write failures and [`Error::JsonParse`] when the
/// event cannot be serialized.
pub fn append_event(root: &Path, event: &RotationEvent) -> Result<()> {
    use std::io::Write as _;

    let path = audit_log_path(root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(event)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

/// Compute the cadence state for `name` given the manifest and audit history.
#[must_use]
pub fn classify_cadence(manifest: &Manifest, events: &[RotationEvent], name: &str) -> Cadence {
    let Some(entry) = manifest.secrets.get(name) else {
        return Cadence::NeverRotated;
    };
    if !rotation_required(entry) {
        return Cadence::NotNeeded;
    }
    let cadence_days = i64::try_from(effective_cadence(manifest, name)).unwrap_or(i64::MAX);
    let last = events
        .iter()
        .filter(|e| e.name == name && matches!(e.outcome, Outcome::Rotated | Outcome::Reminded))
        .map(|e| e.ts)
        .max();
    let Some(last_ts) = last else {
        return Cadence::NeverRotated;
    };
    let now = OffsetDateTime::now_utc();
    let elapsed_days = (now - last_ts).whole_days();
    let remaining = cadence_days - elapsed_days;
    if remaining > 0 {
        Cadence::Healthy {
            days_until_due: remaining,
        }
    } else {
        Cadence::Overdue {
            days_overdue: -remaining,
        }
    }
}

/// Outcome of running one backend.
#[derive(Debug, Clone)]
pub struct RotationOutcome {
    /// Audit-log entry to append.
    pub event: RotationEvent,
    /// Human-readable line for stdout. For self-issued without `target_path`,
    /// contains the new value (printed once).
    pub stdout_line: String,
}

/// Execute the rotation for one secret.
///
/// # Errors
///
/// Returns [`Error::Io`] for filesystem failures, [`Error::Other`] for backend
/// failures (e.g. randomness collection), [`Error::MissingDependency`] for
/// missing exec tools.
pub fn rotate_secret(root: &Path, name: &str, entry: &SecretEntry) -> Result<RotationOutcome> {
    let now = OffsetDateTime::now_utc();
    match entry.backend {
        BackendKind::SelfIssued => rotate_self_issued(root, name, entry, now),
        BackendKind::Manual => Ok(rotate_manual(name, entry, now)),
        BackendKind::Exec => rotate_exec(root, name, entry, now),
        BackendKind::Webhook => rotate_webhook(root, name, entry, now),
    }
}

fn rotate_self_issued(
    root: &Path,
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
) -> Result<RotationOutcome> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| Error::Other(format!("getrandom: {e}")))?;
    let encoded = encode_hex_lower(&bytes);
    let sha = sha256_bytes(encoded.as_bytes());

    finish_value_rotation(
        root,
        name,
        entry,
        now,
        BackendKind::SelfIssued,
        &encoded,
        &sha,
    )
}

fn rotate_exec(
    root: &Path,
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
) -> Result<RotationOutcome> {
    let argv = entry.generate_command.as_deref().ok_or_else(|| {
        Error::contract(format!(
            "[secret.{name}] backend = \"exec\" requires generate_command"
        ))
    })?;
    // Stage tempfile for any {{value_path}} substitutions in generate_command
    // itself (rare but supported). Empty value at this stage; gets rewritten
    // for the deploy phase.
    let staging = tempfile::tempdir().map_err(Error::Io)?;
    let empty_path = staging.path().join("value-stage");
    std::fs::write(&empty_path, b"").map_err(Error::Io)?;

    let substituted = exec::substitute_argv(argv, "", &empty_path);
    let output = exec::run_command(root, entry, &substituted)?;
    if !output.status.success() {
        return Ok(failure_outcome(
            name,
            entry,
            now,
            BackendKind::Exec,
            format!(
                "generate failed: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "no exit code".to_owned(), |c| format!("exit {c}"))
            ),
            None,
            Some(output.sandboxed),
            output.platform_label,
        ));
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() {
        return Ok(failure_outcome(
            name,
            entry,
            now,
            BackendKind::Exec,
            "generate produced empty value".to_owned(),
            None,
            Some(output.sandboxed),
            output.platform_label,
        ));
    }
    let sha = sha256_bytes(value.as_bytes());

    finish_value_rotation(root, name, entry, now, BackendKind::Exec, &value, &sha)
}

fn rotate_webhook(
    root: &Path,
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
) -> Result<RotationOutcome> {
    let result = match crate::webhook::call(entry) {
        Ok(result) => result,
        Err(err) => {
            return Ok(failure_outcome(
                name,
                entry,
                now,
                BackendKind::Webhook,
                err.to_string(),
                None,
                None,
                None,
            ));
        },
    };

    let Some(value) = result.value else {
        // Fire-and-forget trigger: no value, audit as Triggered.
        let stdout_line = format!(
            "triggered {name}: webhook returned HTTP {} (no value captured)",
            result.status
        );
        return Ok(RotationOutcome {
            event: RotationEvent {
                name: name.to_owned(),
                backend: BackendKind::Webhook.as_str().to_owned(),
                ts: now,
                outcome: Outcome::Triggered,
                value_sha256: None,
                target_path: entry.target_path.clone(),
                error: None,
                deploy_count: None,
                deploy_webhook_count: None,
                sandboxed: None,
                platform_sandbox: None,
            },
            stdout_line,
        });
    };

    if value.is_empty() {
        return Ok(failure_outcome(
            name,
            entry,
            now,
            BackendKind::Webhook,
            "webhook response extracted an empty value".to_owned(),
            None,
            None,
            None,
        ));
    }
    let sha = sha256_bytes(value.as_bytes());
    finish_value_rotation(root, name, entry, now, BackendKind::Webhook, &value, &sha)
}

fn rotate_manual(name: &str, entry: &SecretEntry, now: OffsetDateTime) -> RotationOutcome {
    let stdout_line = entry.dashboard_url.as_deref().map_or_else(
        || format!("reminder {name}: rotate manually (no dashboard_url configured)"),
        |url| format!("reminder {name}: rotate manually at {url}"),
    );
    RotationOutcome {
        event: RotationEvent {
            name: name.to_owned(),
            backend: BackendKind::Manual.as_str().to_owned(),
            ts: now,
            outcome: Outcome::Reminded,
            value_sha256: None,
            target_path: None,
            error: None,
            deploy_count: None,
            deploy_webhook_count: None,
            sandboxed: None,
            platform_sandbox: None,
        },
        stdout_line,
    }
}

fn finish_value_rotation(
    root: &Path,
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
    backend: BackendKind,
    value: &str,
    sha: &str,
) -> Result<RotationOutcome> {
    let target_path_str = if let Some(target) = entry.target_path.as_deref() {
        let abs = root.join(target);
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        atomic_write(&abs, format!("{value}\n").as_bytes())?;
        restrict_to_user(&abs)?;
        Some(target.to_owned())
    } else {
        None
    };

    let DeployResult {
        deploy_count,
        deploy_webhook_count,
        sandboxed,
        platform_label,
        failure_error,
    } = run_deploys(root, entry, value)?;

    if let Some(err) = failure_error {
        return Ok(failure_outcome_with_counts(
            name,
            entry,
            now,
            backend,
            err,
            target_path_str,
            sandboxed,
            platform_label,
            deploy_count,
            deploy_webhook_count,
        ));
    }

    let stdout_line = build_stdout_line(name, entry, value, sha, target_path_str.as_deref());

    Ok(RotationOutcome {
        event: RotationEvent {
            name: name.to_owned(),
            backend: backend.as_str().to_owned(),
            ts: now,
            outcome: Outcome::Rotated,
            value_sha256: Some(sha.to_owned()),
            target_path: target_path_str,
            error: None,
            deploy_count,
            deploy_webhook_count,
            sandboxed,
            platform_sandbox: platform_label.map(str::to_owned),
        },
        stdout_line,
    })
}

struct DeployResult {
    deploy_count: Option<u32>,
    deploy_webhook_count: Option<u32>,
    sandboxed: Option<bool>,
    platform_label: Option<&'static str>,
    failure_error: Option<String>,
}

fn run_deploys(root: &Path, entry: &SecretEntry, value: &str) -> Result<DeployResult> {
    let mut result = DeployResult {
        deploy_count: None,
        deploy_webhook_count: None,
        sandboxed: None,
        platform_label: None,
        failure_error: None,
    };

    // Empty arrays are treated as "phase declared, ran 0" so the audit
    // log shape distinguishes "not configured" (None) from "configured
    // but a no-op" (Some(0)).
    if let Some(cmds) = entry.deploy_commands.as_deref() {
        if cmds.is_empty() {
            result.deploy_count = Some(0);
        } else {
            run_deploy_commands(root, entry, value, &mut result)?;
            if result.failure_error.is_some() {
                return Ok(result);
            }
        }
    }

    if let Some(webhooks) = entry.deploy_webhooks.as_deref() {
        if webhooks.is_empty() {
            result.deploy_webhook_count = Some(0);
        } else {
            run_deploy_webhooks(entry, value, &mut result);
        }
    }

    Ok(result)
}

fn run_deploy_commands(
    root: &Path,
    entry: &SecretEntry,
    value: &str,
    result: &mut DeployResult,
) -> Result<()> {
    let deploys = entry
        .deploy_commands
        .as_deref()
        .expect("caller verified has_commands");

    // Stage `{{value_path}}` tempfile under .ready-set/plugins/secrets/tmp/
    // for project-local cleanup. NamedTempFile auto-deletes on drop.
    let stage_dir = root.join(".ready-set/plugins/secrets/tmp");
    std::fs::create_dir_all(&stage_dir).map_err(Error::Io)?;
    let value_file = NamedTempFile::new_in(&stage_dir).map_err(Error::Io)?;
    std::fs::write(value_file.path(), value.as_bytes()).map_err(Error::Io)?;
    restrict_to_user(value_file.path())?;

    let mut success_count: u32 = 0;
    for (i, argv) in deploys.iter().enumerate() {
        let substituted = exec::substitute_argv(argv, value, value_file.path());
        let output: CommandOutput = exec::run_command(root, entry, &substituted)?;
        if result.sandboxed.is_none() {
            result.sandboxed = Some(output.sandboxed);
            result.platform_label = output.platform_label;
        }
        if !output.status.success() {
            let argv0 = argv.first().map_or("(empty)", String::as_str);
            result.deploy_count = Some(success_count);
            result.failure_error = Some(format!(
                "deploy[{i}] `{argv0}` failed: {}",
                output
                    .status
                    .code()
                    .map_or_else(|| "no exit code".to_owned(), |c| format!("exit {c}"))
            ));
            return Ok(());
        }
        success_count += 1;
    }
    result.deploy_count = Some(success_count);
    Ok(())
}

fn run_deploy_webhooks(entry: &SecretEntry, value: &str, result: &mut DeployResult) {
    let webhooks = entry
        .deploy_webhooks
        .as_deref()
        .expect("caller verified has_webhooks");

    let mut success_count: u32 = 0;
    for (i, webhook) in webhooks.iter().enumerate() {
        let outcome = crate::webhook::deploy(webhook, value);
        match outcome {
            Ok(res) if (200..300).contains(&res.status) => {
                success_count += 1;
            },
            Ok(res) => {
                result.deploy_webhook_count = Some(success_count);
                result.failure_error = Some(format!(
                    "deploy_webhook[{i}] {} returned HTTP {}",
                    webhook.url, res.status
                ));
                return;
            },
            Err(err) => {
                result.deploy_webhook_count = Some(success_count);
                result.failure_error = Some(format!("deploy_webhook[{i}] {}: {err}", webhook.url));
                return;
            },
        }
    }
    result.deploy_webhook_count = Some(success_count);
}

fn build_stdout_line(
    name: &str,
    entry: &SecretEntry,
    value: &str,
    sha: &str,
    target_path: Option<&str>,
) -> String {
    let cmd_count = entry
        .deploy_commands
        .as_deref()
        .map_or(0, <[Vec<String>]>::len);
    let webhook_count = entry
        .deploy_webhooks
        .as_deref()
        .map_or(0, <[DeployWebhook]>::len);
    let total_deploys = cmd_count + webhook_count;
    match (target_path, total_deploys) {
        (Some(target), _) => {
            format!("rotated {name}: wrote new value to {target} (sha256 {sha})")
        },
        (None, n) if n > 0 => {
            format!("rotated {name}: pushed to {n} deploy target(s) (sha256 {sha})")
        },
        (None, _) => {
            format!("rotated {name}: {value}  (sha256 {sha})")
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn failure_outcome(
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
    backend: BackendKind,
    error: String,
    target_path: Option<String>,
    sandboxed: Option<bool>,
    platform_label: Option<&'static str>,
) -> RotationOutcome {
    failure_outcome_with_counts(
        name,
        entry,
        now,
        backend,
        error,
        target_path,
        sandboxed,
        platform_label,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn failure_outcome_with_counts(
    name: &str,
    entry: &SecretEntry,
    now: OffsetDateTime,
    backend: BackendKind,
    error: String,
    target_path: Option<String>,
    sandboxed: Option<bool>,
    platform_label: Option<&'static str>,
    deploy_count: Option<u32>,
    deploy_webhook_count: Option<u32>,
) -> RotationOutcome {
    let stdout_line = format!("FAILED {name} ({}): {error}", backend.as_str());
    RotationOutcome {
        event: RotationEvent {
            name: name.to_owned(),
            backend: backend.as_str().to_owned(),
            ts: now,
            outcome: Outcome::Failed,
            value_sha256: None,
            target_path: target_path.or_else(|| entry.target_path.clone()),
            error: Some(error),
            deploy_count,
            deploy_webhook_count,
            sandboxed,
            platform_sandbox: platform_label.map(str::to_owned),
        },
        stdout_line,
    }
}

fn encode_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

mod rfc3339_utc {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    pub fn serialize<S: Serializer>(ts: &OffsetDateTime, ser: S) -> Result<S::Ok, S::Error> {
        ts.format(&Rfc3339)
            .map_err(serde::ser::Error::custom)?
            .serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<OffsetDateTime, D::Error> {
        let raw = String::deserialize(de)?;
        OffsetDateTime::parse(&raw, &Rfc3339).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, ManifestHeader, SecretEntry};

    fn fixed_event(name: &str, seconds_ago: i64) -> RotationEvent {
        RotationEvent {
            name: name.into(),
            backend: "manual".into(),
            ts: OffsetDateTime::now_utc() - time::Duration::seconds(seconds_ago),
            outcome: Outcome::Reminded,
            value_sha256: None,
            target_path: None,
            error: None,
            deploy_count: None,
            deploy_webhook_count: None,
            sandboxed: None,
            platform_sandbox: None,
        }
    }

    #[test]
    fn classify_never_rotated_when_no_events() {
        let m = Manifest::default();
        assert!(matches!(
            classify_cadence(&m, &[], "FOO"),
            Cadence::NeverRotated
        ));
    }

    #[test]
    fn classify_not_needed_when_rotation_disabled() {
        let mut m = Manifest::default();
        m.secrets.insert(
            "FOO".into(),
            SecretEntry {
                rotate: Some(false),
                ..SecretEntry::default()
            },
        );
        let c = classify_cadence(&m, &[], "FOO");
        assert!(matches!(c, Cadence::NotNeeded));
        assert!(!c.needs_rotation());
    }

    #[test]
    fn classify_healthy_when_within_cadence() {
        let mut m = Manifest {
            header: ManifestHeader {
                schema_version: 1,
                default_cadence_days: 90,
            },
            ..Manifest::default()
        };
        m.secrets.insert("FOO".into(), SecretEntry::default());
        let events = vec![fixed_event("FOO", 60 * 60 * 24 * 30)]; // 30d ago
        let c = classify_cadence(&m, &events, "FOO");
        assert!(matches!(c, Cadence::Healthy { .. }));
        assert!(!c.needs_rotation());
    }

    #[test]
    fn classify_overdue_when_past_cadence() {
        let mut m = Manifest {
            header: ManifestHeader {
                schema_version: 1,
                default_cadence_days: 7,
            },
            ..Manifest::default()
        };
        m.secrets.insert("FOO".into(), SecretEntry::default());
        let events = vec![fixed_event("FOO", 60 * 60 * 24 * 60)]; // 60d ago
        let c = classify_cadence(&m, &events, "FOO");
        assert!(matches!(c, Cadence::Overdue { .. }));
        assert!(c.needs_rotation());
    }

    #[test]
    fn append_and_read_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let event = RotationEvent {
            name: "X".into(),
            backend: "manual".into(),
            ts: OffsetDateTime::now_utc(),
            outcome: Outcome::Reminded,
            value_sha256: None,
            target_path: None,
            error: None,
            deploy_count: None,
            deploy_webhook_count: None,
            sandboxed: None,
            platform_sandbox: None,
        };
        append_event(dir.path(), &event).unwrap();
        let events = read_events(dir.path()).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, "X");
    }

    #[test]
    fn read_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = audit_log_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not-json\n{ also not }\n").unwrap();
        let events = read_events(dir.path()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn self_issued_writes_to_target_path_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let entry = SecretEntry {
            backend: BackendKind::SelfIssued,
            target_path: Some("secrets/test-secret".into()),
            ..SecretEntry::default()
        };
        let outcome = rotate_secret(dir.path(), "TEST_SECRET", &entry).unwrap();
        assert!(outcome.event.value_sha256.is_some());
        assert_eq!(
            outcome.event.target_path.as_deref(),
            Some("secrets/test-secret")
        );
        assert_eq!(outcome.event.outcome, Outcome::Rotated);

        let written = std::fs::read_to_string(dir.path().join("secrets/test-secret")).unwrap();
        assert_eq!(written.trim().len(), 64); // 32 bytes hex
        assert!(!outcome.stdout_line.contains(written.trim()));
    }

    #[test]
    fn self_issued_prints_value_when_no_target_path() {
        let dir = tempfile::tempdir().unwrap();
        let entry = SecretEntry {
            backend: BackendKind::SelfIssued,
            ..SecretEntry::default()
        };
        let outcome = rotate_secret(dir.path(), "FOO", &entry).unwrap();
        // The stdout line is where the value goes when no target_path.
        assert!(outcome.stdout_line.contains("FOO"));
        // sha is included in the line.
        let sha = outcome.event.value_sha256.as_ref().unwrap();
        assert!(outcome.stdout_line.contains(sha));
    }

    #[test]
    fn manual_outcome_does_not_set_value_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let entry = SecretEntry {
            backend: BackendKind::Manual,
            dashboard_url: Some("https://example.com".into()),
            ..SecretEntry::default()
        };
        let outcome = rotate_secret(dir.path(), "OPENAI", &entry).unwrap();
        assert_eq!(outcome.event.outcome, Outcome::Reminded);
        assert!(outcome.event.value_sha256.is_none());
        assert!(outcome.stdout_line.contains("https://example.com"));
    }

    #[test]
    fn audit_event_serializes_with_rfc3339() {
        let event = RotationEvent {
            name: "X".into(),
            backend: "manual".into(),
            ts: OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap(),
            outcome: Outcome::Reminded,
            value_sha256: None,
            target_path: None,
            error: None,
            deploy_count: None,
            deploy_webhook_count: None,
            sandboxed: None,
            platform_sandbox: None,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"ts\":\"2023-11-14T22:13:20Z\""));
    }
}
