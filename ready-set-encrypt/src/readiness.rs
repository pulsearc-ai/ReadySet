//! Read-only readiness evaluation for `secrets` and `rotation` capabilities.

use std::path::Path;

use ready_set_sdk::{
    CapabilityRelevance, CapabilityReport, CapabilityState, Error, NextAction, Result,
};

use crate::PROVIDER_ID;
use crate::bundle_cli;
use crate::inventory::{self, Inventory};
use crate::manifest::{self, Manifest};
use crate::rotation::{self, RotationEvent};

/// Evaluate one capability owned by this plugin.
///
/// # Errors
///
/// Forwards filesystem and TOML parsing errors.
pub fn report(capability: &str, root: &Path) -> Result<CapabilityReport> {
    match capability {
        "secrets" => {
            let inv = inventory::scan(root).map_err(Error::Io)?;
            Ok(classify_secrets(&inv))
        },
        "rotation" => {
            let inv = inventory::scan(root).map_err(Error::Io)?;
            let manifest = manifest::load(root)?;
            let events = rotation::read_events(root)?;
            Ok(classify_rotation(&inv, manifest.as_ref(), &events))
        },
        "secret-bundles" => classify_secret_bundles(root),
        other => Err(Error::contract(format!(
            "unknown capability `{other}` for provider `encrypt`"
        ))),
    }
}

#[allow(clippy::too_many_lines)]
fn classify_secret_bundles(root: &Path) -> Result<CapabilityReport> {
    let Some((key_file, key_env, files)) = bundle_cli::configured_files(root).map_err(Error::Io)?
    else {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::NotNeeded,
            "encrypted secret bundles are disabled",
            None,
        ));
    };
    if files.is_empty() {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::Missing,
            "bundle config is enabled but no bundle files are configured",
            Some(set_action(
                "ready-set encrypt",
                "Add bundle file mappings to .ready-set/plugins/secrets/config.toml",
            )),
        ));
    }
    if key_file.is_none() && std::env::var(&key_env).is_err() {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::Missing,
            format!("bundle key is not available in {key_env}"),
            Some(set_action(
                "ready-set encrypt key generate",
                "Generate a key once, save it outside this device, and provide it at runtime",
            )),
        ));
    }
    if let Some(key_file) = key_file.as_ref()
        && !key_file.is_file()
        && std::env::var(&key_env).is_err()
    {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::Missing,
            format!(
                "explicit local bundle key is missing at {}",
                display(root, key_file)
            ),
            Some(set_action(
                "ready-set encrypt",
                "Create the explicit key file or provide the saved key at runtime",
            )),
        ));
    }
    let key = match bundle_cli::load_configured_key(key_file.as_deref(), &key_env) {
        Ok((key, _source)) => key,
        Err(err) => {
            return Ok(base_report(
                "secret-bundles",
                "Secret Bundles",
                CapabilityState::Blocked,
                format!("bundle key is invalid: {err}"),
                Some(set_action(
                    "ready-set encrypt key generate",
                    "Restore the saved key or generate a new one and re-encrypt bundles",
                )),
            ));
        },
    };

    let mut missing = Vec::new();
    let mut failed = Vec::new();
    for file in &files {
        if !root.join(&file.source).is_file() {
            failed.push(format!("{} source missing", file.source));
            continue;
        }
        if !root.join(&file.encrypted).is_file() {
            missing.push(file.encrypted.clone());
            continue;
        }
        if let Err(err) = bundle_cli::verify_configured(root, &key, file) {
            failed.push(format!("{}: {err}", file.encrypted));
        }
    }

    if !failed.is_empty() {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::Blocked,
            format!(
                "{} bundle check(s) failed: {}",
                failed.len(),
                truncated_list(&failed)
            ),
            Some(set_action(
                "ready-set encrypt",
                "Re-encrypt configured bundle files",
            )),
        ));
    }
    if !missing.is_empty() {
        return Ok(base_report(
            "secret-bundles",
            "Secret Bundles",
            CapabilityState::Missing,
            format!(
                "{} encrypted bundle(s) missing: {}",
                missing.len(),
                truncated_list(&missing)
            ),
            Some(set_action(
                "ready-set encrypt",
                "Encrypt configured plaintext files",
            )),
        ));
    }

    Ok(base_report(
        "secret-bundles",
        "Secret Bundles",
        CapabilityState::Ready,
        format!(
            "{} encrypted bundle(s) decrypt and match source keys",
            files.len()
        ),
        None,
    ))
}

fn classify_secrets(inv: &Inventory) -> CapabilityReport {
    if inv.referenced.is_empty() && inv.declared.is_empty() && inv.local.is_empty() {
        return base_report(
            "secrets",
            "Secrets",
            CapabilityState::NotNeeded,
            "no environment variables detected in this project",
            None,
        );
    }

    if !inv.env_example_present {
        return base_report(
            "secrets",
            "Secrets",
            CapabilityState::Missing,
            ".env.example is missing",
            Some(set_action(
                "ready-set set secrets",
                "Create .env.example and ignore rules from detected env vars",
            )),
        );
    }

    let missing = inv.missing_from_example();
    let orphans = inv.orphans_in_example();
    match (missing.is_empty(), orphans.is_empty()) {
        (true, true) => base_report(
            "secrets",
            "Secrets",
            CapabilityState::Ready,
            ".env.example matches detected env vars",
            None,
        ),
        (false, _) => base_report(
            "secrets",
            "Secrets",
            CapabilityState::Incomplete,
            format!(
                "{} env var(s) referenced in code but missing from .env.example: {}",
                missing.len(),
                truncated_list(&missing)
            ),
            Some(set_action(
                "ready-set set secrets",
                "Reconcile .env.example with detected env vars",
            )),
        ),
        (true, false) => base_report(
            "secrets",
            "Secrets",
            CapabilityState::Stale,
            format!(
                "{} env var(s) in .env.example not referenced in code: {}",
                orphans.len(),
                truncated_list(&orphans)
            ),
            Some(set_action(
                "ready-set set --force secrets",
                "Prune orphans from .env.example",
            )),
        ),
    }
}

fn classify_rotation(
    inv: &Inventory,
    manifest: Option<&Manifest>,
    events: &[RotationEvent],
) -> CapabilityReport {
    let detected = inv.all_names();
    if detected.is_empty() {
        return base_report(
            "rotation",
            "Rotation",
            CapabilityState::NotNeeded,
            "no environment variables detected; nothing to rotate",
            None,
        );
    }

    let Some(manifest) = manifest else {
        return base_report(
            "rotation",
            "Rotation",
            CapabilityState::Blocked,
            "rotation manifest is missing",
            Some(set_action(
                "ready-set set secrets",
                "Scaffold .ready-set/plugins/secrets/manifest.toml",
            )),
        );
    };

    let manifest_names: std::collections::BTreeSet<String> =
        manifest.secrets.keys().cloned().collect();
    let drift: Vec<String> = detected.difference(&manifest_names).cloned().collect();
    let stale: Vec<String> = manifest_names.difference(&detected).cloned().collect();
    if !drift.is_empty() {
        return base_report(
            "rotation",
            "Rotation",
            CapabilityState::Incomplete,
            format!(
                "{} env var(s) detected but missing from rotation manifest: {}",
                drift.len(),
                truncated_list(&drift)
            ),
            Some(set_action(
                "ready-set set secrets",
                "Reconcile rotation manifest with detected env vars",
            )),
        );
    }
    if !stale.is_empty() {
        return base_report(
            "rotation",
            "Rotation",
            CapabilityState::Incomplete,
            format!(
                "{} manifest secret(s) not in canonical inventory: {}",
                stale.len(),
                truncated_list(&stale)
            ),
            Some(set_action(
                "ready-set set --force secrets",
                "Prune stale rotation manifest entries",
            )),
        );
    }

    let overdue: Vec<String> = manifest
        .secrets
        .keys()
        .filter(|name| rotation::classify_cadence(manifest, events, name).needs_rotation())
        .cloned()
        .collect();

    if overdue.is_empty() {
        base_report(
            "rotation",
            "Rotation",
            CapabilityState::Ready,
            "all rotation-tracked manifest secrets within cadence",
            None,
        )
    } else {
        base_report(
            "rotation",
            "Rotation",
            CapabilityState::Stale,
            format!(
                "{} secret(s) overdue or never rotated: {}",
                overdue.len(),
                truncated_list(&overdue)
            ),
            Some(set_action(
                "ready-set rotate --confirm",
                "Rotate overdue secrets (--confirm required for irreversible action)",
            )),
        )
    }
}

fn truncated_list(items: &[String]) -> String {
    const MAX: usize = 5;
    if items.len() <= MAX {
        items.join(", ")
    } else {
        let head = items[..MAX].join(", ");
        format!("{head}, … (+{} more)", items.len() - MAX)
    }
}

fn base_report(
    id: &str,
    title: &str,
    state: CapabilityState,
    summary: impl Into<String>,
    next_action: Option<NextAction>,
) -> CapabilityReport {
    CapabilityReport {
        id: id.into(),
        title: title.into(),
        provider: PROVIDER_ID.into(),
        state,
        relevance: CapabilityRelevance::Required,
        summary: summary.into(),
        next_action,
    }
}

fn set_action(command: impl Into<String>, description: impl Into<String>) -> NextAction {
    NextAction {
        command: command.into(),
        description: description.into(),
    }
}

fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{BackendKind, ManifestHeader, SecretEntry};
    use crate::rotation::Outcome;
    use std::collections::BTreeSet;
    use time::OffsetDateTime;

    fn names(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn not_needed_when_no_env_vars_anywhere() {
        let inv = Inventory::default();
        let r = classify_secrets(&inv);
        assert_eq!(r.state, CapabilityState::NotNeeded);
        assert!(r.next_action.is_none());
    }

    #[test]
    fn missing_when_example_absent_but_refs_exist() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let r = classify_secrets(&inv);
        assert_eq!(r.state, CapabilityState::Missing);
    }

    #[test]
    fn ready_when_example_matches_refs() {
        let inv = Inventory {
            declared: names(&["FOO"]),
            referenced: names(&["FOO"]),
            env_example_present: true,
            ..Inventory::default()
        };
        let r = classify_secrets(&inv);
        assert_eq!(r.state, CapabilityState::Ready);
        assert!(r.next_action.is_none());
    }

    #[test]
    fn incomplete_when_refs_exceed_example() {
        let inv = Inventory {
            declared: names(&["FOO"]),
            referenced: names(&["FOO", "BAR"]),
            env_example_present: true,
            ..Inventory::default()
        };
        let r = classify_secrets(&inv);
        assert_eq!(r.state, CapabilityState::Incomplete);
        assert!(r.summary.contains("BAR"));
    }

    #[test]
    fn stale_when_example_has_orphans() {
        let inv = Inventory {
            declared: names(&["FOO", "EXTRA"]),
            referenced: names(&["FOO"]),
            env_example_present: true,
            ..Inventory::default()
        };
        let r = classify_secrets(&inv);
        assert_eq!(r.state, CapabilityState::Stale);
        assert!(r.summary.contains("EXTRA"));
    }

    #[test]
    fn rejects_unknown_capability() {
        let dir = tempfile::tempdir().unwrap();
        let err = report("bogus", dir.path()).unwrap_err();
        assert!(matches!(err, Error::ContractViolation(_)));
    }

    #[test]
    fn rotation_not_needed_with_empty_inventory() {
        let inv = Inventory::default();
        let r = classify_rotation(&inv, None, &[]);
        assert_eq!(r.state, CapabilityState::NotNeeded);
    }

    #[test]
    fn rotation_blocked_without_manifest() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let r = classify_rotation(&inv, None, &[]);
        assert_eq!(r.state, CapabilityState::Blocked);
        assert!(r.next_action.unwrap().command.contains("set secrets"));
    }

    #[test]
    fn rotation_incomplete_when_manifest_drifts_from_inventory() {
        let inv = Inventory {
            referenced: names(&["FOO", "BAR"]),
            ..Inventory::default()
        };
        let mut manifest = Manifest::default();
        manifest
            .secrets
            .insert("FOO".into(), SecretEntry::default());
        let r = classify_rotation(&inv, Some(&manifest), &[]);
        assert_eq!(r.state, CapabilityState::Incomplete);
        assert!(r.summary.contains("BAR"));
    }

    #[test]
    fn rotation_incomplete_when_manifest_has_stale_entries() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let mut manifest = Manifest::default();
        manifest
            .secrets
            .insert("FOO".into(), SecretEntry::default());
        manifest
            .secrets
            .insert("STALE".into(), SecretEntry::default());

        let r = classify_rotation(&inv, Some(&manifest), &[]);

        assert_eq!(r.state, CapabilityState::Incomplete);
        assert!(r.summary.contains("STALE"));
        assert!(r.next_action.unwrap().command.contains("--force"));
    }

    #[test]
    fn rotation_stale_when_secret_never_rotated() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let mut manifest = Manifest::default();
        manifest
            .secrets
            .insert("FOO".into(), SecretEntry::default());
        let r = classify_rotation(&inv, Some(&manifest), &[]);
        assert_eq!(r.state, CapabilityState::Stale);
        assert!(r.next_action.unwrap().command.contains("--confirm"));
    }

    #[test]
    fn rotation_ready_when_manifest_entry_disables_rotation() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let mut manifest = Manifest::default();
        manifest.secrets.insert(
            "FOO".into(),
            SecretEntry {
                backend: BackendKind::Manual,
                rotate: Some(false),
                ..SecretEntry::default()
            },
        );

        let r = classify_rotation(&inv, Some(&manifest), &[]);

        assert_eq!(r.state, CapabilityState::Ready);
        assert!(r.next_action.is_none());
    }

    #[test]
    fn rotation_ready_when_recent_audit_within_cadence() {
        let inv = Inventory {
            referenced: names(&["FOO"]),
            ..Inventory::default()
        };
        let mut manifest = Manifest {
            header: ManifestHeader {
                schema_version: 1,
                default_cadence_days: 365,
            },
            ..Manifest::default()
        };
        manifest.secrets.insert(
            "FOO".into(),
            SecretEntry {
                backend: BackendKind::Manual,
                ..SecretEntry::default()
            },
        );
        let events = vec![RotationEvent {
            name: "FOO".into(),
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
        }];
        let r = classify_rotation(&inv, Some(&manifest), &events);
        assert_eq!(r.state, CapabilityState::Ready);
        assert!(r.next_action.is_none());
    }
}
