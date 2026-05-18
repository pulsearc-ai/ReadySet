//! Read-only readiness evaluation for Rust capabilities.

use std::path::Path;

use ready_set_sdk::{
    CapabilityRelevance, CapabilityReport, CapabilityState, Error, NextAction, Result,
};
use toml_edit::{DocumentMut, Item};

use crate::PROVIDER_ID;
use crate::gitignore;
use crate::manifest_edit;
use crate::members;
use crate::templates::{CLIPPY, RUST_TOOLCHAIN, RUSTFMT};
use crate::workspace::Workspace;

/// Evaluate one Rust capability.
///
/// # Errors
///
/// Forwards filesystem and TOML parsing errors.
pub fn report(capability: &str, workspace: Option<&Workspace>) -> Result<CapabilityReport> {
    let Some(workspace) = workspace else {
        return base_report(
            capability,
            CapabilityState::Blocked,
            "no Cargo manifest found",
            None,
        );
    };

    match capability {
        "workspace" => workspace_report(workspace),
        "toolchain" => Ok(template_file_report(
            capability,
            &workspace.root,
            "rust-toolchain.toml",
            RUST_TOOLCHAIN,
        )?),
        "formatting" => Ok(template_file_report(
            capability,
            &workspace.root,
            "rustfmt.toml",
            RUSTFMT,
        )?),
        "linting" => linting_report(workspace),
        other => Err(Error::contract(format!(
            "unknown Rust capability `{other}`"
        ))),
    }
}

fn workspace_report(workspace: &Workspace) -> Result<CapabilityReport> {
    let raw = std::fs::read_to_string(&workspace.manifest_path)?;
    let doc = parse_manifest(&raw, workspace)?;

    let workspace_ready = workspace_table_ready(&doc, &members::discover(&workspace.root));
    let gitignore_ready = std::fs::read_to_string(workspace.root.join(".gitignore"))
        .ok()
        .is_some_and(|current| gitignore::plan(Some(&current)).is_none());
    let config_ready = workspace.root.join(".ready-set.toml").is_file();

    if workspace_ready && gitignore_ready && config_ready {
        base_report(
            "workspace",
            CapabilityState::Ready,
            "workspace configuration is ready",
            None,
        )
    } else {
        base_report(
            "workspace",
            CapabilityState::Missing,
            "workspace setup is missing",
            Some(set_action(
                "ready-set set workspace",
                "Create the workspace configuration",
            )),
        )
    }
}

fn template_file_report(
    capability: &str,
    root: &Path,
    file_name: &'static str,
    template: &str,
) -> Result<CapabilityReport> {
    let path = root.join(file_name);
    let Ok(current) = std::fs::read_to_string(path) else {
        return base_report(
            capability,
            CapabilityState::Missing,
            format!("{file_name} is missing"),
            Some(set_action(
                format!("ready-set set {capability}"),
                format!("Create the {capability} configuration"),
            )),
        );
    };

    if current == template {
        base_report(
            capability,
            CapabilityState::Ready,
            format!("{file_name} is ready"),
            None,
        )
    } else {
        base_report(
            capability,
            CapabilityState::Stale,
            format!("{file_name} differs from template"),
            Some(set_action(
                format!("ready-set set --force {capability}"),
                format!("Reconcile the {capability} configuration"),
            )),
        )
    }
}

fn linting_report(workspace: &Workspace) -> Result<CapabilityReport> {
    let clippy = std::fs::read_to_string(workspace.root.join("clippy.toml")).ok();
    let raw = std::fs::read_to_string(&workspace.manifest_path)?;
    let doc = parse_manifest(&raw, workspace)?;
    let lints_match = manifest_edit::workspace_lints_match(&doc)?;

    if clippy.is_none() || lints_match.is_none() {
        return base_report(
            "linting",
            CapabilityState::Missing,
            "linting configuration is missing",
            Some(set_action(
                "ready-set set linting",
                "Create the linting configuration",
            )),
        );
    }

    if clippy.as_deref() == Some(CLIPPY) && lints_match == Some(true) {
        base_report(
            "linting",
            CapabilityState::Ready,
            "linting configuration is ready",
            None,
        )
    } else {
        base_report(
            "linting",
            CapabilityState::Stale,
            "linting configuration differs from template",
            Some(set_action(
                "ready-set set --force linting",
                "Reconcile the linting configuration",
            )),
        )
    }
}

fn workspace_table_ready(doc: &DocumentMut, desired_members: &[String]) -> bool {
    let Some(workspace) = doc.get("workspace").and_then(Item::as_table) else {
        return false;
    };
    if workspace.get("resolver").and_then(Item::as_str) != Some("3") {
        return false;
    }
    let Some(members) = workspace.get("members").and_then(Item::as_array) else {
        return false;
    };
    desired_members.iter().all(|desired| {
        members
            .iter()
            .any(|member| member.as_str() == Some(desired.as_str()))
    })
}

fn base_report(
    id: &str,
    state: CapabilityState,
    summary: impl Into<String>,
    next_action: Option<NextAction>,
) -> Result<CapabilityReport> {
    Ok(CapabilityReport {
        id: id.into(),
        title: title_for(id)?.into(),
        provider: PROVIDER_ID.into(),
        state,
        relevance: CapabilityRelevance::Required,
        summary: summary.into(),
        next_action,
    })
}

fn title_for(id: &str) -> Result<&'static str> {
    match id {
        "workspace" => Ok("Workspace"),
        "toolchain" => Ok("Toolchain"),
        "formatting" => Ok("Formatting"),
        "linting" => Ok("Linting"),
        other => Err(Error::contract(format!(
            "unknown Rust capability `{other}`"
        ))),
    }
}

fn set_action(command: impl Into<String>, description: impl Into<String>) -> NextAction {
    NextAction {
        command: command.into(),
        description: description.into(),
    }
}

fn parse_manifest(raw: &str, workspace: &Workspace) -> Result<DocumentMut> {
    raw.parse().map_err(|err: toml_edit::TomlError| {
        Error::TomlParse(format!("{}: {err}", workspace.manifest_path.display()))
    })
}
