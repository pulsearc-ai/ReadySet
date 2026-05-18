//! Setup/reconciliation execution for Rust capabilities.

use std::path::{Path, PathBuf};

use ready_set_sdk::change_log::{ChangeLog, ChangeOp, ChangeRecord, backup_file};
use ready_set_sdk::fs::{atomic_write, sha256_bytes, sha256_file};
use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRunReport, CapabilityVerb, Context, Error,
    Output, OutputMode, Result, RunStatus,
};
use time::OffsetDateTime;
use toml_edit::DocumentMut;

use crate::PROVIDER_ID;
use crate::gitignore;
use crate::manifest_edit;
use crate::members;
use crate::options::SetOptions;
use crate::ready_set_toml;
use crate::templates::{CLIPPY, RUST_TOOLCHAIN, RUSTFMT};
use crate::workspace::Workspace;

struct PlannedWrite {
    abs: PathBuf,
    content: String,
    rel: String,
    op: ChangeOp,
    before_content: Option<String>,
}

/// Execute `set` for one Rust capability.
///
/// # Errors
///
/// Forwards SDK errors from I/O, TOML parsing, or change-log writes.
pub fn set_capability(
    capability: &str,
    workspace: &Workspace,
    opts: &SetOptions,
    ctx: &Context,
) -> Result<CapabilityRunReport> {
    let mut actions = Vec::new();
    let mut planned_writes = Vec::new();

    match capability {
        "workspace" => plan_workspace(workspace, opts, &mut actions, &mut planned_writes)?,
        "toolchain" => plan_template_file(
            &workspace.root,
            "rust-toolchain.toml",
            RUST_TOOLCHAIN,
            opts.force,
            &mut actions,
            &mut planned_writes,
        ),
        "formatting" => plan_template_file(
            &workspace.root,
            "rustfmt.toml",
            RUSTFMT,
            opts.force,
            &mut actions,
            &mut planned_writes,
        ),
        "linting" => plan_linting(workspace, opts, &mut actions, &mut planned_writes)?,
        other => {
            return Err(Error::contract(format!(
                "unknown Rust capability `{other}`"
            )));
        },
    }

    execute_planned_writes(workspace, opts, &mut actions, &planned_writes)?;

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

fn plan_workspace(
    workspace: &Workspace,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) -> Result<()> {
    let gi_path = workspace.root.join(".gitignore");
    let gi_current = std::fs::read_to_string(&gi_path).ok();
    if let Some(new_content) = gitignore::plan(gi_current.as_deref()) {
        let op = if gi_current.is_some() {
            ChangeOp::Modify
        } else {
            ChangeOp::Create
        };
        planned_writes.push(PlannedWrite {
            abs: gi_path,
            content: new_content,
            rel: ".gitignore".into(),
            op,
            before_content: gi_current,
        });
    } else {
        actions.push(check(".gitignore", "managed block already up to date"));
    }

    let config_path = workspace.root.join(".ready-set.toml");
    if config_path.is_file() {
        actions.push(check(".ready-set.toml", "already exists"));
    } else {
        planned_writes.push(PlannedWrite {
            abs: config_path,
            content: ready_set_toml::render(
                workspace.has_workspace_table || workspace.has_package_table,
            ),
            rel: ".ready-set.toml".into(),
            op: ChangeOp::Create,
            before_content: None,
        });
    }

    let manifest_raw = std::fs::read_to_string(&workspace.manifest_path)?;
    let mut doc: DocumentMut = manifest_raw.parse().map_err(|e: toml_edit::TomlError| {
        Error::TomlParse(format!("{}: {e}", workspace.manifest_path.display()))
    })?;
    let mut desired_members = Vec::new();
    if !opts.no_discover {
        desired_members.extend(members::discover(&workspace.root));
    }
    desired_members.extend(opts.members.clone());
    let plan = manifest_edit::apply_workspace(&mut doc, &desired_members)?;
    if plan.changed {
        planned_writes.push(PlannedWrite {
            abs: workspace.manifest_path.clone(),
            content: doc.to_string(),
            rel: "Cargo.toml".into(),
            op: ChangeOp::Modify,
            before_content: Some(manifest_raw),
        });
    } else {
        actions.push(check("Cargo.toml", "workspace already up to date"));
    }
    for member in plan.added_members {
        actions.push(CapabilityAction {
            kind: CapabilityActionKind::Check,
            summary: format!("workspace member planned: {member}"),
            path: Some("Cargo.toml".into()),
        });
    }

    Ok(())
}

fn plan_linting(
    workspace: &Workspace,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) -> Result<()> {
    plan_template_file(
        &workspace.root,
        "clippy.toml",
        CLIPPY,
        opts.force,
        actions,
        planned_writes,
    );

    let manifest_raw = std::fs::read_to_string(&workspace.manifest_path)?;
    let mut doc: DocumentMut = manifest_raw.parse().map_err(|e: toml_edit::TomlError| {
        Error::TomlParse(format!("{}: {e}", workspace.manifest_path.display()))
    })?;
    let plan = manifest_edit::apply_lints(&mut doc, opts.force)?;
    if plan.changed {
        planned_writes.push(PlannedWrite {
            abs: workspace.manifest_path.clone(),
            content: doc.to_string(),
            rel: "Cargo.toml".into(),
            op: ChangeOp::Modify,
            before_content: Some(manifest_raw),
        });
    } else if plan.lints_drifted {
        actions.push(skip(
            "Cargo.toml",
            "workspace lints differ from template; pass --force to overwrite",
        ));
    } else {
        actions.push(check("Cargo.toml", "workspace lints already up to date"));
    }

    Ok(())
}

fn plan_template_file(
    root: &Path,
    rel: &'static str,
    template: &str,
    force: bool,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &mut Vec<PlannedWrite>,
) {
    let abs = root.join(rel);
    let current = std::fs::read_to_string(&abs).ok();
    if let Some(current) = current {
        if current == template {
            actions.push(check(rel, "already up to date"));
        } else if force {
            planned_writes.push(PlannedWrite {
                abs,
                content: template.into(),
                rel: rel.into(),
                op: ChangeOp::Modify,
                before_content: Some(current),
            });
        } else {
            actions.push(skip(
                rel,
                "differs from template; pass --force to overwrite",
            ));
        }
    } else {
        planned_writes.push(PlannedWrite {
            abs,
            content: template.into(),
            rel: rel.into(),
            op: ChangeOp::Create,
            before_content: None,
        });
    }
}

fn execute_planned_writes(
    workspace: &Workspace,
    opts: &SetOptions,
    actions: &mut Vec<CapabilityAction>,
    planned_writes: &[PlannedWrite],
) -> Result<()> {
    let mut log = if opts.dry_run || planned_writes.is_empty() {
        None
    } else {
        Some(ChangeLog::open(&workspace.root, PROVIDER_ID)?)
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
            backup_file(&workspace.root, &write.abs)?;
        }
        atomic_write(&write.abs, write.content.as_bytes())?;
        let after_sha = sha256_file(&write.abs)?;
        if let Some(log) = log.as_mut() {
            log.record(&ChangeRecord {
                op: write.op,
                path: relative_path(&workspace.root, &write.abs),
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
        out.human(&format!("ready-set-rust set {capability} (dry-run)"));
    } else {
        out.human(&format!("ready-set-rust set {capability}"));
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
