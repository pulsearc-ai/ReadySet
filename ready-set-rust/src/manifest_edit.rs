//! Edit the root `Cargo.toml` to register workspace state and lints.

use std::collections::BTreeSet;

use ready_set_sdk::{Error, Result};
use toml_edit::{Array, DocumentMut, Item, Table};

use crate::templates::WORKSPACE_LINTS;

/// Plan of workspace edits.
#[derive(Debug, Default, Clone)]
pub struct WorkspacePlan {
    /// Whether the document was changed.
    pub changed: bool,
    /// Members added to `[workspace.members]`.
    pub added_members: Vec<String>,
    /// Whether the `[workspace]` table was added.
    pub workspace_table_added: bool,
}

/// Plan of lint edits.
#[derive(Debug, Default, Clone)]
pub struct LintsPlan {
    /// Whether the document was changed.
    pub changed: bool,
    /// Whether `[workspace.lints.*]` was rewritten.
    pub lints_updated: bool,
    /// Whether existing lints differ from the template but `--force` was not
    /// requested.
    pub lints_drifted: bool,
}

/// Apply workspace edits to `doc`.
///
/// # Errors
///
/// Returns [`Error::Other`] if `[workspace]` exists but is not a table.
pub fn apply_workspace(doc: &mut DocumentMut, desired_members: &[String]) -> Result<WorkspacePlan> {
    let mut plan = WorkspacePlan::default();

    if doc.get("workspace").is_none() {
        doc["workspace"] = Item::Table(Table::new());
        plan.workspace_table_added = true;
        plan.changed = true;
    }
    let workspace = doc["workspace"]
        .as_table_mut()
        .ok_or_else(|| Error::Other("[workspace] is not a table".into()))?;

    if workspace.get("resolver").and_then(Item::as_str) != Some("3") {
        workspace["resolver"] = toml_edit::value("3");
        plan.changed = true;
    }

    let mut existing: BTreeSet<String> = BTreeSet::new();
    if let Some(arr) = workspace.get("members").and_then(Item::as_array) {
        for value in arr {
            if let Some(member) = value.as_str() {
                existing.insert(member.to_string());
            }
        }
    }
    let mut merged = existing.clone();
    for member in desired_members {
        merged.insert(member.clone());
    }
    if merged != existing {
        let mut arr = Array::new();
        for member in &merged {
            arr.push(member.as_str());
            if !existing.contains(member) {
                plan.added_members.push(member.clone());
            }
        }
        workspace["members"] = toml_edit::value(arr);
        plan.changed = true;
    } else if workspace.get("members").is_none() {
        workspace["members"] = toml_edit::value(Array::new());
        plan.changed = true;
    }

    Ok(plan)
}

/// Apply workspace lint edits to `doc`.
///
/// # Errors
///
/// Returns [`Error::TomlParse`] if the lints template fails to parse or
/// [`Error::Other`] if `[workspace]` exists but is not a table.
pub fn apply_lints(doc: &mut DocumentMut, force: bool) -> Result<LintsPlan> {
    let mut plan = LintsPlan::default();
    if doc.get("workspace").is_none() {
        doc["workspace"] = Item::Table(Table::new());
        plan.changed = true;
    }
    let workspace = doc["workspace"]
        .as_table_mut()
        .ok_or_else(|| Error::Other("[workspace] is not a table".into()))?;

    let template_doc: DocumentMut =
        WORKSPACE_LINTS.parse().map_err(|e: toml_edit::TomlError| {
            Error::TomlParse(format!("workspace-lints template: {e}"))
        })?;
    let template_lints = template_doc["workspace"]["lints"].clone();

    match workspace.get("lints") {
        None => {
            workspace["lints"] = template_lints;
            plan.lints_updated = true;
            plan.changed = true;
        },
        Some(existing) if items_structurally_equal(existing, &template_lints) => {},
        Some(_) if force => {
            workspace["lints"] = template_lints;
            plan.lints_updated = true;
            plan.changed = true;
        },
        Some(_) => {
            plan.lints_drifted = true;
        },
    }

    Ok(plan)
}

/// Compare an existing document's `[workspace.lints]` table to the built-in
/// lints template.
///
/// # Errors
///
/// Returns [`Error::TomlParse`] if the canonical lints template fails to
/// parse.
pub fn workspace_lints_match(doc: &DocumentMut) -> Result<Option<bool>> {
    let Some(workspace) = doc.get("workspace").and_then(Item::as_table) else {
        return Ok(None);
    };
    let Some(existing) = workspace.get("lints") else {
        return Ok(None);
    };
    let template_doc: DocumentMut =
        WORKSPACE_LINTS.parse().map_err(|e: toml_edit::TomlError| {
            Error::TomlParse(format!("workspace-lints template: {e}"))
        })?;
    let template_lints = &template_doc["workspace"]["lints"];

    Ok(Some(items_structurally_equal(existing, template_lints)))
}

fn items_structurally_equal(a: &Item, b: &Item) -> bool {
    let to_value = |item: &Item| -> Option<toml::Value> {
        let raw = format!("k = {item}");
        match item {
            Item::Table(_) | Item::ArrayOfTables(_) => {
                let mut wrapper = String::from("[wrapper]\n");
                let body = item.to_string();
                if body.is_empty() {
                    return None;
                }
                wrapper.push_str(&body);
                toml::from_str::<toml::Value>(&wrapper).ok()
            },
            _ => match toml::from_str::<toml::Value>(&raw).ok()? {
                toml::Value::Table(t) => t.into_iter().next().map(|(_, v)| v),
                _ => None,
            },
        }
    };

    match (to_value(a), to_value(b)) {
        (Some(x), Some(y)) => x == y,
        _ => tables_equal(a, b),
    }
}

fn tables_equal(a: &Item, b: &Item) -> bool {
    match (a, b) {
        (Item::Table(ta), Item::Table(tb)) => {
            let keys_a: BTreeSet<&str> = ta.iter().map(|(k, _)| k).collect();
            let keys_b: BTreeSet<&str> = tb.iter().map(|(k, _)| k).collect();
            if keys_a != keys_b {
                return false;
            }
            for key in &keys_a {
                let Some(va) = ta.get(key) else { return false };
                let Some(vb) = tb.get(key) else { return false };
                if !tables_equal(va, vb) {
                    return false;
                }
            }
            true
        },
        (Item::Value(va), Item::Value(vb)) => va.to_string().trim() == vb.to_string().trim(),
        _ => false,
    }
}
