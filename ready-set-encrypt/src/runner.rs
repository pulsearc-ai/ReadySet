//! Setup/reconciliation execution for the `secrets` capability.

use std::path::{Path, PathBuf};

use ready_set_sdk::change_log::{ChangeLog, ChangeOp, ChangeRecord, backup_file};
use ready_set_sdk::fs::{atomic_write, sha256_bytes, sha256_file};
use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRunReport, CapabilityVerb, Context, Error,
    Output, OutputMode, Result, RunStatus,
};
use time::OffsetDateTime;

use crate::PROVIDER_ID;
use crate::bundle_cli;
use crate::inventory::{self, Inventory, split_env_example};
use crate::manifest;
use crate::options::SetOptions;
use crate::scaffold::{self, gitleaks_template};

struct PlannedWrite {
    abs: PathBuf,
    content: String,
    rel: String,
    op: ChangeOp,
    before_content: Option<String>,
}

/// Execute `set` for the secrets capability.
///
/// # Errors
///
/// Forwards SDK errors from I/O or change-log writes.
pub fn set_capability(
    capability: &str,
    root: &Path,
    opts: &SetOptions,
    ctx: &Context,
) -> Result<CapabilityRunReport> {
    if capability == "secret-bundles" {
        return set_secret_bundles(root, opts, ctx);
    }
    if capability != "secrets" {
        return Err(Error::contract(format!(
            "unknown secrets capability `{capability}`"
        )));
    }

    let mut actions = Vec::new();
    let mut planned_writes = Vec::new();
    let inv = inventory::scan(root).map_err(Error::Io)?;

    plan_env_example(root, &inv, opts, &mut actions, &mut planned_writes);
    plan_gitignore_block(root, &mut actions, &mut planned_writes);
    plan_gitleaks(root, opts, &mut actions, &mut planned_writes);
    plan_canonical_template(root, &inv, &mut actions, &mut planned_writes);
    plan_rotation_manifest(root, &inv, opts, &mut actions, &mut planned_writes)?;

    execute_planned_writes(root, opts, &mut actions, &planned_writes)?;

    let status = if planned_writes.is_empty() || opts.dry_run {
        RunStatus::Noop
    } else {
        RunStatus::Changed
    };
    let report = CapabilityRunReport {
        id: capability.into(),
        verb: CapabilityVerb::Set,
        status,
        actions,
    };
    render_report(capability, &report, opts, ctx)?;
    Ok(report)
}

#[allow(clippy::too_many_lines)]
fn set_secret_bundles(
    root: &Path,
    opts: &SetOptions,
    ctx: &Context,
) -> Result<CapabilityRunReport> {
    let mut actions = Vec::new();
    let Some((key_file, key_env, files)) = bundle_cli::configured_files(root).map_err(Error::Io)?
    else {
        let report = CapabilityRunReport {
            id: "secret-bundles".into(),
            verb: CapabilityVerb::Set,
            status: RunStatus::Noop,
            actions: vec![skip("secret-bundles", "encrypted bundles are disabled")],
        };
        render_report("secret-bundles", &report, opts, ctx)?;
        return Ok(report);
    };

    let key = if std::env::var(&key_env).is_ok() {
        match bundle_cli::load_configured_key(None, &key_env) {
            Ok((key, source)) => {
                actions.push(check(source, "bundle key available"));
                Some(key)
            },
            Err(err) => {
                actions.push(error("bundle key", err));
                None
            },
        }
    } else if let Some(key_file) = key_file.as_ref() {
        if key_file.is_file() {
            actions.push(check(
                display(root, key_file),
                "explicit local bundle key exists",
            ));
            match crate::bundle::load_local_key_file(key_file) {
                Ok(key) => Some(key),
                Err(err) => {
                    actions.push(error(
                        display(root, key_file),
                        format!("invalid local key: {err}"),
                    ));
                    None
                },
            }
        } else if opts.dry_run {
            actions.push(skip(
                display(root, key_file),
                "would create explicit local bundle key file",
            ));
            None
        } else {
            match crate::bundle::create_local_key_file(key_file, "local") {
                Ok(key) => {
                    actions.push(CapabilityAction {
                        kind: CapabilityActionKind::Create,
                        summary: "created explicit local bundle key file; save a backup somewhere safe or encrypted bundles cannot be recovered if this key is lost".into(),
                        path: Some(display(root, key_file)),
                    });
                    Some(key)
                },
                Err(create_err) => {
                    actions.push(error(display(root, key_file), create_err.to_string()));
                    None
                },
            }
        }
    } else {
        actions.push(error(
            "bundle key",
            format!(
                "bundle key not available; run `ready-set encrypt key generate` once and provide the saved value via {key_env}"
            ),
        ));
        None
    };

    if files.is_empty() {
        actions.push(skip("secret-bundles", "no bundle files configured"));
    }

    if let Some(key) = key.as_ref() {
        for file in &files {
            let source = root.join(&file.source);
            if !source.is_file() {
                actions.push(error(&file.source, "source file missing"));
                continue;
            }
            if opts.dry_run {
                actions.push(skip(
                    &file.encrypted,
                    if file.redact_source {
                        format!("would encrypt {} and redact source values", file.source)
                    } else {
                        format!("would encrypt {}", file.source)
                    },
                ));
                continue;
            }
            let existed = root.join(&file.encrypted).is_file();
            if existed
                && bundle_cli::configured_plaintext_matches(root, key, file)
                    .is_ok_and(|matches| matches)
            {
                actions.push(check(
                    &file.encrypted,
                    "encrypted bundle already up to date",
                ));
                continue;
            }
            let change_summary = bundle_change_summary(root, key, file, existed);
            match bundle_cli::encrypt_configured(root, key, file) {
                Ok(()) => {
                    actions.push(CapabilityAction {
                        kind: if existed {
                            CapabilityActionKind::Modify
                        } else {
                            CapabilityActionKind::Create
                        },
                        summary: change_summary,
                        path: Some(file.encrypted.clone()),
                    });
                    if file.redact_source {
                        match bundle_cli::redact_configured_source(root, file) {
                            Ok(true) => actions.push(CapabilityAction {
                                kind: CapabilityActionKind::Modify,
                                summary: "redacted plaintext values after encryption".into(),
                                path: Some(file.source.clone()),
                            }),
                            Ok(false) => {
                                actions.push(check(&file.source, "source already redacted"));
                            },
                            Err(err) => actions.push(error(&file.source, err)),
                        }
                    }
                },
                Err(err) => actions.push(error(&file.encrypted, err)),
            }
        }
    }

    let has_error = actions
        .iter()
        .any(|action| action.kind == CapabilityActionKind::Error);
    let changed = actions.iter().any(|action| {
        matches!(
            action.kind,
            CapabilityActionKind::Create | CapabilityActionKind::Modify
        )
    });
    let status = if has_error {
        RunStatus::Failed
    } else if changed && !opts.dry_run {
        RunStatus::Changed
    } else {
        RunStatus::Noop
    };
    let report = CapabilityRunReport {
        id: "secret-bundles".into(),
        verb: CapabilityVerb::Set,
        status,
        actions,
    };
    render_report("secret-bundles", &report, opts, ctx)?;
    Ok(report)
}

fn bundle_change_summary(
    root: &Path,
    key: &crate::bundle::LocalKey,
    file: &crate::config::BundleFileConfig,
    existed: bool,
) -> String {
    if existed {
        return match bundle_cli::configured_effective_plaintext_diff(root, key, file) {
            Ok(diff) if diff.is_empty() => {
                format!(
                    "encrypted from {} (no dotenv key/value changes)",
                    file.source
                )
            },
            Ok(diff) => {
                let mut parts = Vec::new();
                if !diff.added.is_empty() {
                    parts.push(format!("added: {}", truncated_list(&diff.added)));
                }
                if !diff.changed.is_empty() {
                    parts.push(format!("changed: {}", truncated_list(&diff.changed)));
                }
                if !diff.removed.is_empty() {
                    parts.push(format!("removed: {}", truncated_list(&diff.removed)));
                }
                if !diff.exposed.is_empty() {
                    parts.push(format!(
                        "plaintext exposed: {}",
                        truncated_list(&diff.exposed)
                    ));
                }
                format!("encrypted from {} ({})", file.source, parts.join("; "))
            },
            Err(err) => format!(
                "encrypted from {} (previous bundle diff unavailable: {err})",
                file.source
            ),
        };
    }

    match bundle_cli::configured_source_keys(root, file) {
        Ok(keys) if keys.is_empty() => format!("encrypted from {} (0 dotenv keys)", file.source),
        Ok(keys) => format!(
            "encrypted from {} (new keys: {})",
            file.source,
            truncated_list(&keys)
        ),
        Err(err) => format!(
            "encrypted from {} (source key list unavailable: {err})",
            file.source
        ),
    }
}

fn plan_canonical_template(
    root: &Path,
    inv: &Inventory,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) {
    let detected = inv.all_names();
    if detected.is_empty() {
        return;
    }

    let path = root.join("deploy/secrets/canonical.env.template");
    let current = std::fs::read_to_string(&path).ok();
    let desired = scaffold::render_canonical_env_template(&detected);
    match current {
        None => planned_writes.push(PlannedWrite {
            abs: path,
            content: desired,
            rel: "deploy/secrets/canonical.env.template".into(),
            op: ChangeOp::Create,
            before_content: None,
        }),
        Some(existing) if existing == desired => {
            actions.push(check(
                "deploy/secrets/canonical.env.template",
                "already up to date",
            ));
        },
        Some(existing) => planned_writes.push(PlannedWrite {
            abs: path,
            content: desired,
            rel: "deploy/secrets/canonical.env.template".into(),
            op: ChangeOp::Modify,
            before_content: Some(existing),
        }),
    }
}

fn plan_env_example(
    root: &Path,
    inv: &Inventory,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) {
    let path = root.join(".env.example");
    let current = std::fs::read_to_string(&path).ok();

    let all_detected = inv.all_names();
    if all_detected.is_empty() {
        actions.push(check(
            ".env.example",
            "no env vars detected; nothing to scaffold",
        ));
        return;
    }

    let (prelude, user_keys) = current.as_deref().map_or_else(
        || (String::new(), std::collections::BTreeSet::new()),
        split_env_example,
    );
    let desired = scaffold::render_env_example(&prelude, &user_keys, &all_detected);

    match current {
        None => planned_writes.push(PlannedWrite {
            abs: path,
            content: desired,
            rel: ".env.example".into(),
            op: ChangeOp::Create,
            before_content: None,
        }),
        Some(existing) if existing == desired => {
            actions.push(check(".env.example", "already up to date"));
        },
        Some(existing) => {
            let orphans = inv.orphans_in_example();
            if !orphans.is_empty() && !opts.force {
                actions.push(skip(
                    ".env.example",
                    format!(
                        "{} orphan(s) detected ({}); pass --force to prune",
                        orphans.len(),
                        truncated_list(&orphans)
                    ),
                ));
                let desired_keep_orphans = scaffold::render_env_example(
                    &prelude,
                    &user_keys,
                    &inv.all_names().union(&inv.declared).cloned().collect(),
                );
                if existing != desired_keep_orphans {
                    planned_writes.push(PlannedWrite {
                        abs: path,
                        content: desired_keep_orphans,
                        rel: ".env.example".into(),
                        op: ChangeOp::Modify,
                        before_content: Some(existing),
                    });
                }
            } else {
                planned_writes.push(PlannedWrite {
                    abs: path,
                    content: desired,
                    rel: ".env.example".into(),
                    op: ChangeOp::Modify,
                    before_content: Some(existing),
                });
            }
        },
    }
}

fn plan_gitignore_block(
    root: &Path,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) {
    let path = root.join(".gitignore");
    let current = std::fs::read_to_string(&path).ok();
    if let Some(new_content) = scaffold::plan_gitignore(current.as_deref()) {
        let op = if current.is_some() {
            ChangeOp::Modify
        } else {
            ChangeOp::Create
        };
        planned_writes.push(PlannedWrite {
            abs: path,
            content: new_content,
            rel: ".gitignore".into(),
            op,
            before_content: current,
        });
    } else {
        actions.push(check(".gitignore", "managed block already up to date"));
    }
}

fn plan_gitleaks(
    root: &Path,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) {
    let path = root.join(".gitleaks.toml");
    let current = std::fs::read_to_string(&path).ok();
    let template = gitleaks_template();
    match current {
        None => planned_writes.push(PlannedWrite {
            abs: path,
            content: template.into(),
            rel: ".gitleaks.toml".into(),
            op: ChangeOp::Create,
            before_content: None,
        }),
        Some(existing) if existing == template => {
            actions.push(check(".gitleaks.toml", "already up to date"));
        },
        Some(existing) => {
            if opts.force {
                planned_writes.push(PlannedWrite {
                    abs: path,
                    content: template.into(),
                    rel: ".gitleaks.toml".into(),
                    op: ChangeOp::Modify,
                    before_content: Some(existing),
                });
            } else {
                actions.push(skip(
                    ".gitleaks.toml",
                    "differs from template; pass --force to overwrite",
                ));
            }
        },
    }
}

fn plan_rotation_manifest(
    root: &Path,
    inv: &Inventory,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) -> Result<()> {
    let detected = inv.all_names();
    if detected.is_empty() {
        return Ok(());
    }

    let path = manifest::manifest_path(root);
    let rel = ".ready-set/plugins/secrets/manifest.toml".to_owned();

    match manifest::load_document(root)? {
        None => {
            planned_writes.push(PlannedWrite {
                abs: path,
                content: manifest::render_initial(&detected),
                rel,
                op: ChangeOp::Create,
                before_content: None,
            });
        },
        Some((mut doc, before_raw)) => {
            let added = manifest::add_missing_secrets(&mut doc, &detected);
            let removed = if opts.force {
                manifest::prune_stale_secrets(&mut doc, &detected)
            } else {
                std::collections::BTreeSet::default()
            };
            if added.is_empty() && removed.is_empty() {
                actions.push(check(rel, "rotation manifest up to date"));
            } else {
                planned_writes.push(PlannedWrite {
                    abs: path,
                    content: doc.to_string(),
                    rel,
                    op: ChangeOp::Modify,
                    before_content: Some(before_raw),
                });
            }
        },
    }
    Ok(())
}

fn execute_planned_writes(
    root: &Path,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &[PlannedWrite],
) -> Result<()> {
    let mut log = if opts.dry_run || planned_writes.is_empty() {
        None
    } else {
        Some(ChangeLog::open(root, PROVIDER_ID)?)
    };

    for write in planned_writes {
        if opts.dry_run {
            actions.push(skip(&write.rel, "plan only"));
            continue;
        }

        let before_sha = write
            .before_content
            .as_ref()
            .map(|prev| sha256_bytes(prev.as_bytes()));
        if matches!(write.op, ChangeOp::Modify) && write.abs.is_file() {
            backup_file(root, &write.abs)?;
        }
        atomic_write(&write.abs, write.content.as_bytes())?;
        let after_sha = sha256_file(&write.abs)?;
        if let Some(log) = log.as_mut() {
            log.record(&ChangeRecord {
                op: write.op,
                path: relative_path(root, &write.abs),
                before_sha256: before_sha,
                after_sha256: Some(after_sha),
                ts: OffsetDateTime::now_utc(),
            })?;
        }
        actions.push(CapabilityAction {
            kind: match write.op {
                ChangeOp::Create => CapabilityActionKind::Create,
                ChangeOp::Modify => CapabilityActionKind::Modify,
                ChangeOp::Delete => CapabilityActionKind::Delete,
            },
            summary: "written".into(),
            path: Some(write.rel.clone()),
        });
    }

    Ok(())
}

fn render_report(
    capability: &str,
    report: &CapabilityRunReport,
    opts: &SetOptions,
    ctx: &Context,
) -> Result<()> {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        out.json(report)?;
        return Ok(());
    }

    if opts.dry_run {
        out.human(&format!("ready-set-encrypt set {capability} (dry-run)"));
    } else {
        out.human(&format!("ready-set-encrypt set {capability}"));
    }
    for action in &report.actions {
        let path = action.path.as_deref().unwrap_or("-");
        out.human(&format!(
            "  {:<8} {:<24} {}",
            action_kind_label(action.kind),
            path,
            action.summary
        ));
    }
    Ok(())
}

fn truncated_list(items: &[String]) -> String {
    const MAX: usize = 3;
    if items.len() <= MAX {
        items.join(", ")
    } else {
        format!("{}, …", items[..MAX].join(", "))
    }
}

fn check(path: impl Into<String>, summary: impl Into<String>) -> CapabilityAction {
    CapabilityAction {
        kind: CapabilityActionKind::Check,
        summary: summary.into(),
        path: Some(path.into()),
    }
}

fn skip(path: impl Into<String>, summary: impl Into<String>) -> CapabilityAction {
    CapabilityAction {
        kind: CapabilityActionKind::Skip,
        summary: summary.into(),
        path: Some(path.into()),
    }
}

fn error(path: impl Into<String>, summary: impl Into<String>) -> CapabilityAction {
    CapabilityAction {
        kind: CapabilityActionKind::Error,
        summary: summary.into(),
        path: Some(path.into()),
    }
}

fn display(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

const fn action_kind_label(kind: CapabilityActionKind) -> &'static str {
    match kind {
        CapabilityActionKind::Create => "create",
        CapabilityActionKind::Modify => "modify",
        CapabilityActionKind::Delete => "delete",
        CapabilityActionKind::Run => "run",
        CapabilityActionKind::Check => "check",
        CapabilityActionKind::Skip => "skip",
        CapabilityActionKind::Error => "error",
    }
}

fn relative_path(root: &Path, abs: &Path) -> PathBuf {
    abs.strip_prefix(root).map_or_else(
        |_| abs.to_path_buf(),
        |path| PathBuf::from(path.to_string_lossy().replace('\\', "/")),
    )
}
