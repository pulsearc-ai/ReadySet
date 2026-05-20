//! End-to-end tests for the `ready-set-rust` provider plugin.

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Command;

use ready_set_sdk::change_log::{ChangeOp, reverse_dir};

const fn plugin() -> &'static str {
    env!("CARGO_BIN_EXE_ready-set-rust")
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

#[cfg(unix)]
fn fake_cargo(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("cargo");
    write(
        &bin,
        r#"#!/bin/sh
if [ -n "$READY_SET_FAKE_CARGO_LOG" ]; then
  printf '%s\n' "$*" >> "$READY_SET_FAKE_CARGO_LOG"
fi
if [ -n "$READY_SET_FAKE_CARGO_EXIT" ]; then
  exit "$READY_SET_FAKE_CARGO_EXIT"
fi
exit 0
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    bin
}

#[test]
fn describe_emits_four_rust_capabilities() {
    let out = Command::new(plugin()).arg("__describe").output().unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let capabilities = parsed["capabilities"].as_array().unwrap();
    assert_eq!(capabilities.len(), 4);
    let ids: Vec<&str> = capabilities
        .iter()
        .map(|capability| capability["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"workspace"));
    assert!(ids.contains(&"toolchain"));
    assert!(ids.contains(&"formatting"));
    assert!(ids.contains(&"linting"));

    let verbs = |id: &str| -> Vec<&str> {
        capabilities
            .iter()
            .find(|capability| capability["id"] == id)
            .unwrap()["verbs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|verb| verb.as_str().unwrap())
            .collect()
    };
    assert!(!verbs("workspace").contains(&"go"));
    assert!(!verbs("toolchain").contains(&"go"));
    assert!(verbs("formatting").contains(&"go"));
    assert!(verbs("linting").contains(&"go"));
}

#[test]
fn ready_reports_missing_for_unconfigured_toolchain() {
    let dir = fresh_workspace();
    let out = Command::new(plugin())
        .args(["__ready", "toolchain"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.id.as_str(), "toolchain");
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Missing);
}

#[test]
fn set_formatting_writes_only_rustfmt() {
    let dir = fresh_workspace();
    let out = Command::new(plugin())
        .args(["__set", "formatting"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(dir.path().join("rustfmt.toml").is_file());
    assert!(!dir.path().join("rust-toolchain.toml").exists());
    assert!(!dir.path().join("clippy.toml").exists());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.id.as_str(), "formatting");
}

#[test]
fn dry_run_writes_nothing() {
    let dir = fresh_workspace();
    let initial_cargo = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    let out = Command::new(plugin())
        .args(["__set", "workspace", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(!dir.path().join(".ready-set").exists());
    assert!(!dir.path().join(".ready-set.toml").exists());
    let after = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert_eq!(after, initial_cargo);
}

#[test]
fn force_overwrites_diverged_formatting_file() {
    let dir = fresh_workspace();
    write(&dir.path().join("rustfmt.toml"), "edition = \"2021\"\n");
    let out = Command::new(plugin())
        .args(["__set", "formatting", "--force"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let after = std::fs::read_to_string(dir.path().join("rustfmt.toml")).unwrap();
    assert!(after.contains("edition                    = \"2024\""));
}

#[test]
fn linting_without_force_does_not_overwrite_custom_workspace_lints() {
    let dir = fresh_workspace();
    write(
        &dir.path().join("Cargo.toml"),
        "[workspace]\nresolver = \"3\"\nmembers = []\n\n[workspace.lints.rust]\nunsafe_code = \"warn\"\n",
    );
    let out = Command::new(plugin())
        .args(["__set", "linting"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let manifest = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
    assert!(manifest.contains("unsafe_code = \"warn\""));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert!(report.actions.iter().any(|action| {
        action.kind == ready_set_sdk::CapabilityActionKind::Skip
            && action.path.as_deref() == Some("Cargo.toml")
            && action.summary.contains("pass --force")
    }));
}

#[test]
fn changelog_uses_rust_provider_name() {
    let dir = fresh_workspace();
    let out = Command::new(plugin())
        .args(["__set", "toolchain"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let changes_dir = dir.path().join(".ready-set/changes");
    let entries: Vec<_> = std::fs::read_dir(&changes_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1);
    let name = entries[0].file_name();
    assert!(name.to_string_lossy().starts_with("rust-"));
}

#[test]
fn set_linting_changelog_can_restore_workspace() {
    let dir = fresh_workspace();
    let manifest_path = dir.path().join("Cargo.toml");
    let original_manifest = std::fs::read_to_string(&manifest_path).unwrap();
    let out = Command::new(plugin())
        .args(["__set", "linting"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(dir.path().join("clippy.toml").is_file());
    assert_ne!(
        std::fs::read_to_string(&manifest_path).unwrap(),
        original_manifest
    );

    let records = reverse_dir(dir.path()).unwrap();
    assert_eq!(records.len(), 2);
    let clippy_record = records
        .iter()
        .find(|(_, record)| record.path == Path::new("clippy.toml"))
        .unwrap()
        .1
        .clone();
    assert_eq!(clippy_record.op, ChangeOp::Create);
    assert!(clippy_record.before_sha256.is_none());
    assert!(clippy_record.after_sha256.is_some());

    let cargo_record = records
        .iter()
        .find(|(_, record)| record.path == Path::new("Cargo.toml"))
        .unwrap()
        .1
        .clone();
    assert_eq!(cargo_record.op, ChangeOp::Modify);
    let before_sha = cargo_record.before_sha256.as_ref().unwrap();
    let backup = dir.path().join(".ready-set/backups").join(before_sha);
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original_manifest);
    assert!(cargo_record.after_sha256.is_some());

    for (_, record) in records {
        let path = dir.path().join(&record.path);
        match record.op {
            ChangeOp::Create => std::fs::remove_file(path).unwrap(),
            ChangeOp::Modify => {
                let before_sha = record.before_sha256.as_ref().unwrap();
                let backup = dir.path().join(".ready-set/backups").join(before_sha);
                std::fs::copy(backup, path).unwrap();
            },
            ChangeOp::Delete => unreachable!("set linting does not delete files"),
        }
    }

    assert!(!dir.path().join("clippy.toml").exists());
    assert_eq!(
        std::fs::read_to_string(manifest_path).unwrap(),
        original_manifest
    );
}

#[cfg(unix)]
#[test]
fn go_formatting_runs_cargo_fmt_check() {
    let dir = fresh_workspace();
    let bin_dir = tempfile::tempdir().unwrap();
    drop(fake_cargo(bin_dir.path()));
    let log = dir.path().join("cargo.log");
    let out = Command::new(plugin())
        .args(["__go", "formatting"])
        .env("PATH", bin_dir.path())
        .env("READY_SET_OUTPUT", "json")
        .env("READY_SET_FAKE_CARGO_LOG", &log)
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(log).unwrap(), "fmt --check\n");
    assert!(!dir.path().join(".ready-set").exists());
    assert!(!dir.path().join("rustfmt.toml").exists());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.id.as_str(), "formatting");
    assert_eq!(report.verb, ready_set_sdk::CapabilityVerb::Go);
    assert_eq!(report.status, ready_set_sdk::RunStatus::Ok);
}

#[cfg(unix)]
#[test]
fn go_linting_runs_cargo_clippy_workspace_all_targets() {
    let dir = fresh_workspace();
    let bin_dir = tempfile::tempdir().unwrap();
    drop(fake_cargo(bin_dir.path()));
    let log = dir.path().join("cargo.log");
    let out = Command::new(plugin())
        .args(["__go", "linting"])
        .env("PATH", bin_dir.path())
        .env("READY_SET_OUTPUT", "json")
        .env("READY_SET_FAKE_CARGO_LOG", &log)
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(log).unwrap(),
        "clippy --workspace --all-targets\n"
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.id.as_str(), "linting");
    assert_eq!(report.status, ready_set_sdk::RunStatus::Ok);
}

#[cfg(unix)]
#[test]
fn go_failure_emits_failed_json_report() {
    let dir = fresh_workspace();
    let bin_dir = tempfile::tempdir().unwrap();
    drop(fake_cargo(bin_dir.path()));
    let out = Command::new(plugin())
        .args(["__go", "formatting"])
        .env("PATH", bin_dir.path())
        .env("READY_SET_OUTPUT", "json")
        .env("READY_SET_FAKE_CARGO_EXIT", "1")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Failed);
    assert_eq!(
        report.actions[0].kind,
        ready_set_sdk::CapabilityActionKind::Error
    );
}

#[test]
fn go_workspace_is_user_error() {
    let dir = fresh_workspace();
    let out = Command::new(plugin())
        .args(["__go", "workspace"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("capability `workspace` does not support go"));
}
