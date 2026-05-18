//! Cargo workspace member discovery.

use std::collections::BTreeSet;
use std::path::Path;

use walkdir::WalkDir;

const MAX_DEPTH: usize = 6;

/// Discover candidate workspace members under `root`.
///
/// Scans for directories containing a `Cargo.toml`. Skips `target/`, hidden
/// directories, and nested workspaces.
#[must_use]
pub fn discover(root: &Path) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    let mut walker = WalkDir::new(root).max_depth(MAX_DEPTH).into_iter();

    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_dir() {
            continue;
        }
        if is_skipped(entry.path(), root) {
            walker.skip_current_dir();
            continue;
        }

        let manifest = entry.path().join("Cargo.toml");
        if !manifest.is_file() || entry.path() == root {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(&manifest)
            && raw.contains("[workspace]")
        {
            walker.skip_current_dir();
            continue;
        }

        if let Ok(rel) = entry.path().strip_prefix(root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            if !s.is_empty() {
                out.insert(s);
            }
        }
    }

    out.into_iter().collect()
}

fn is_skipped(path: &Path, root: &Path) -> bool {
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    if name == "target" || name == "node_modules" || name == ".git" {
        return true;
    }
    name.starts_with('.') && name != "." && name != ".."
}
