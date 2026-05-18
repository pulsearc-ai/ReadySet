//! PATH discovery of `ready-set-<name>` plugin binaries.
//!
//! On Unix the dispatcher walks `PATH` for files named `ready-set-<name>`.
//! On Windows it honors `PATHEXT` and matches `ready-set-<name>.exe` (and
//! other declared executable extensions).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ready_set_sdk::manifest::Manifest;

const PREFIX: &str = "ready-set-";

/// One entry on PATH that looks like a plugin.
#[derive(Debug, Clone)]
pub struct PluginEntry {
    /// Plugin subcommand name (no `ready-set-` prefix and no extension).
    pub name: String,
    /// Absolute path to the binary.
    pub binary_path: PathBuf,
    /// Manifest sidecar, if present alongside the binary.
    pub manifest: Option<Manifest>,
}

/// Find the first plugin on PATH whose name matches `name`. Returns `None`
/// if no match exists.
#[must_use]
pub fn find_plugin(name: &str) -> Option<PluginEntry> {
    if name.is_empty() {
        return None;
    }
    let target_name = format!("{PREFIX}{name}");
    for dir in path_dirs() {
        if let Some(p) = scan_dir_for_match(&dir, &target_name) {
            let manifest = Manifest::sibling_of(&p)
                .canonicalize()
                .ok()
                .and_then(|m| Manifest::load(&m).ok());
            return Some(PluginEntry {
                name: name.to_string(),
                binary_path: p,
                manifest,
            });
        }
    }
    None
}

/// Enumerate every `ready-set-*` binary on PATH, dedup by canonical path,
/// load sidecar manifests when present.
#[must_use]
pub fn list_all() -> Vec<PluginEntry> {
    let mut seen: BTreeSet<PathBuf> = BTreeSet::new();
    let mut out: Vec<PluginEntry> = Vec::new();
    for dir in path_dirs() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.flatten() {
            let p = entry.path();
            let Some(file_name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let stem = strip_executable_suffix(file_name);
            let Some(name) = stem.strip_prefix(PREFIX) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            if !is_executable(&p) {
                continue;
            }
            let canon = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
            if !seen.insert(canon.clone()) {
                continue;
            }
            let manifest = Manifest::sibling_of(&p)
                .canonicalize()
                .ok()
                .and_then(|m| Manifest::load(&m).ok());
            out.push(PluginEntry {
                name: name.to_string(),
                binary_path: p,
                manifest,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn path_dirs() -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path).collect()
}

fn scan_dir_for_match(dir: &Path, target_stem: &str) -> Option<PathBuf> {
    for ext in candidate_extensions() {
        let candidate = if ext.is_empty() {
            dir.join(target_stem)
        } else {
            dir.join(format!("{target_stem}{ext}"))
        };
        if candidate.is_file() && is_executable(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn candidate_extensions() -> Vec<String> {
    if cfg!(windows) {
        let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut out: Vec<String> = raw
            .split(';')
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        if out.is_empty() {
            out.push(".exe".into());
        }
        out
    } else {
        vec![String::new()]
    }
}

fn strip_executable_suffix(file_name: &str) -> &str {
    if cfg!(windows) {
        let lower = file_name.to_ascii_lowercase();
        for ext in candidate_extensions() {
            if lower.ends_with(&ext) {
                return &file_name[..file_name.len() - ext.len()];
            }
        }
    }
    file_name
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && (m.permissions().mode() & 0o111 != 0))
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_is_not_a_plugin() {
        assert!(find_plugin("").is_none());
    }

    #[test]
    fn list_all_does_not_panic() {
        // Smoke test only; a fresh env may have no plugins on PATH.
        drop(list_all());
    }
}
