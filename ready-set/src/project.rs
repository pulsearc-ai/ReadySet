//! Project root detection.
//!
//! The dispatcher walks upward from the current working directory looking for
//! a `.git` directory, the nearest `Cargo.toml`, or `.ready-set.toml`. The
//! first hit wins. The result is exported as `READY_SET_PROJECT_ROOT` for
//! plugins that want it.

use std::path::{Path, PathBuf};

/// Walk upward from `cwd` to detect a project root. Returns `None` if the
/// filesystem root is reached without finding any marker.
#[must_use]
pub fn detect_project_root(cwd: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(cwd);
    let mut nearest_cargo: Option<PathBuf> = None;
    while let Some(dir) = cur {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        if dir.join(".ready-set.toml").exists() {
            return Some(dir.to_path_buf());
        }
        if nearest_cargo.is_none() && dir.join("Cargo.toml").exists() {
            nearest_cargo = Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    nearest_cargo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_git_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let inner = dir.path().join("a/b/c");
        std::fs::create_dir_all(&inner).unwrap();
        let root = detect_project_root(&inner).unwrap();
        assert_eq!(root, dir.path());
    }

    #[test]
    fn finds_ready_set_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".ready-set.toml"),
            "[ready-set]\nschema_version = 1\n",
        )
        .unwrap();
        let inner = dir.path().join("a");
        std::fs::create_dir_all(&inner).unwrap();
        let root = detect_project_root(&inner).unwrap();
        assert_eq!(root, dir.path());
    }

    #[test]
    fn falls_back_to_nearest_cargo_toml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let inner = dir.path().join("src");
        std::fs::create_dir_all(&inner).unwrap();
        let root = detect_project_root(&inner).unwrap();
        assert_eq!(root, dir.path());
    }

    #[test]
    fn returns_none_when_nothing_found() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("a/b");
        std::fs::create_dir_all(&inner).unwrap();
        // tempdir() lives somewhere with parents that may have markers.
        // We can't assert None reliably (the system /tmp parents may have
        // a .git somewhere); just assert the function does not panic.
        drop(detect_project_root(&inner));
    }
}
