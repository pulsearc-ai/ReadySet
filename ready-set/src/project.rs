//! Project root detection.
//!
//! The dispatcher walks upward from the current working directory looking for
//! the nearest `.ready-set.toml`, then a `.git` directory. The result is
//! exported as `READY_SET_PROJECT_ROOT` for plugins that want it. Language-
//! specific roots, such as Cargo workspaces, are provider concerns.

use std::path::{Path, PathBuf};

/// Walk upward from `cwd` to detect a project root. Returns `None` if the
/// filesystem root is reached without finding any marker.
#[must_use]
pub fn detect_project_root(cwd: &Path) -> Option<PathBuf> {
    let mut cur: Option<&Path> = Some(cwd);
    let mut nearest_git: Option<PathBuf> = None;
    while let Some(dir) = cur {
        if dir.join(".ready-set.toml").exists() {
            return Some(dir.to_path_buf());
        }
        if nearest_git.is_none() && dir.join(".git").exists() {
            nearest_git = Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    nearest_git
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
    fn ignores_language_specific_manifests() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname=\"x\"\n").unwrap();
        let inner = dir.path().join("src");
        std::fs::create_dir_all(&inner).unwrap();
        assert!(detect_project_root(&inner).is_none());
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
