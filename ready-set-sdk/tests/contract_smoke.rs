//! End-to-end smoke tests for SDK contract types.

use std::path::PathBuf;

use ready_set_sdk::change_log::{ChangeLog, ChangeOp, ChangeRecord, backup_file, reverse_dir};
use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::manifest::Manifest;

#[test]
fn describe_serializes_to_one_line_json() {
    let d = Describe {
        description: "smoke".into(),
        version: "0.1.0".parse().unwrap(),
        stability: Stability::Stable,
        min_dispatcher_version: "0.1.0".parse().unwrap(),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        project_requirements: Vec::new(),
        capabilities: Vec::new(),
        command_aliases: Vec::new(),
    };
    let json = serde_json::to_string(&d).unwrap();
    assert!(!json.contains('\n'));
    let back: Describe = serde_json::from_str(&json).unwrap();
    assert_eq!(d, back);
}

#[test]
fn manifest_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ready-set-test.toml");
    std::fs::write(
        &path,
        r#"
description              = "smoke"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
capabilities             = []
"#,
    )
    .unwrap();

    let m = Manifest::load(&path).unwrap();
    assert_eq!(m.description, "smoke");
    assert_eq!(m.platforms.len(), 3);
}

#[test]
fn change_log_records_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    let src = root.join("file.txt");
    std::fs::write(&src, b"original").unwrap();
    let before_sha = backup_file(root, &src).unwrap();
    std::fs::write(&src, b"updated").unwrap();
    let after_sha = ready_set_sdk::fs::sha256_file(&src).unwrap();

    let mut log = ChangeLog::open(root, "smoke").unwrap();
    let record = ChangeRecord {
        op: ChangeOp::Modify,
        path: PathBuf::from("file.txt"),
        before_sha256: Some(before_sha.clone()),
        after_sha256: Some(after_sha.clone()),
        ts: time::OffsetDateTime::now_utc(),
    };
    log.record(&record).unwrap();
    drop(log);

    let all = reverse_dir(root).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].1.op, ChangeOp::Modify);
    assert_eq!(all[0].1.before_sha256, Some(before_sha));
    assert_eq!(all[0].1.after_sha256, Some(after_sha));
}
