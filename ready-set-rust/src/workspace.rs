//! Cargo workspace resolution.

use std::path::{Path, PathBuf};

use ready_set_sdk::{Error, Result};
use toml_edit::DocumentMut;

/// Resolved Cargo manifest hosting the workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Project root containing `Cargo.toml`.
    pub root: PathBuf,
    /// Path to that `Cargo.toml`.
    pub manifest_path: PathBuf,
    /// Whether the manifest already has a `[workspace]` table.
    pub has_workspace_table: bool,
    /// Whether the manifest has a `[package]` table.
    pub has_package_table: bool,
}

/// Walk up from `cwd` to find the nearest Cargo project.
///
/// # Errors
///
/// Returns [`Error::Io`] if a manifest cannot be read or
/// [`Error::TomlParse`] if it does not parse.
pub fn resolve(cwd: &Path) -> Result<Option<Workspace>> {
    let mut nearest_manifest: Option<PathBuf> = None;
    let mut cur: Option<&Path> = Some(cwd);

    while let Some(dir) = cur {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() && nearest_manifest.is_none() {
            nearest_manifest = Some(manifest);
        }
        if dir.join(".git").exists() {
            break;
        }
        cur = dir.parent();
    }

    let Some(manifest_path) = nearest_manifest else {
        return Ok(None);
    };

    let raw = std::fs::read_to_string(&manifest_path)?;
    let doc: DocumentMut = raw.parse().map_err(|e: toml_edit::TomlError| {
        Error::TomlParse(format!("{}: {e}", manifest_path.display()))
    })?;

    let has_workspace_table = doc.get("workspace").is_some();
    let has_package_table = doc.get("package").is_some();

    if !has_workspace_table && !has_package_table {
        return Ok(None);
    }

    let root = manifest_path
        .parent()
        .ok_or_else(|| Error::Other(format!("{} has no parent", manifest_path.display())))?
        .to_path_buf();

    Ok(Some(Workspace {
        root,
        manifest_path,
        has_workspace_table,
        has_package_table,
    }))
}
