//! End-to-end tests for core `ready-set go` behavior.

use std::path::Path;
use std::process::Command;

const fn dispatcher() -> &'static str {
    env!("CARGO_BIN_EXE_ready-set")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn fresh_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    write(
        &dir.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = []\n",
    );
    dir
}

#[test]
fn go_without_go_capabilities_is_read_only_user_error() {
    let dir = fresh_workspace();

    let status = Command::new(dispatcher())
        .arg("go")
        .current_dir(dir.path())
        .status()
        .unwrap();

    assert_eq!(status.code(), Some(1));
    assert!(!dir.path().join("rust-toolchain.toml").exists());
    assert!(!dir.path().join("rustfmt.toml").exists());
    assert!(!dir.path().join("clippy.toml").exists());
    assert!(!dir.path().join(".ready-set.toml").exists());
    assert!(!dir.path().join(".ready-set/changes").exists());
}
