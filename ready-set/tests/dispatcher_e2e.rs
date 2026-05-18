//! End-to-end tests for the dispatcher binary.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

fn dispatcher_bin() -> PathBuf {
    // Set by cargo when running integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_ready-set"))
}

#[test]
fn version_prints_semver() {
    let out = Command::new(dispatcher_bin())
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.starts_with("ready-set "));
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn unknown_subcommand_exits_127() {
    let out = Command::new(dispatcher_bin())
        .arg("totallyfakething")
        .env_remove("RUST_LOG")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(127));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown subcommand"));
}

#[test]
fn list_includes_builtins_in_human_mode() {
    // Use an empty PATH override to avoid any plugins from the test machine.
    let out = Command::new(dispatcher_bin())
        .arg("--list")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    for name in ["ready", "set", "go", "help", "version", "list"] {
        assert!(stdout.contains(name), "{stdout}");
    }
    assert!(!stdout.contains("undo"), "{stdout}");
    let stale_bootstrap = "Bootstrap".to_string() + " a Rust workspace";
    assert!(!stdout.contains(&stale_bootstrap), "{stdout}");
}

#[test]
fn list_json_emits_valid_json_array() {
    let out = Command::new(dispatcher_bin())
        .arg("--json")
        .arg("--list")
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(parsed.is_array());
    let names: Vec<String> = parsed
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap().to_string())
        .collect();
    for name in ["ready", "set", "go", "help", "version", "list"] {
        assert!(names.contains(&name.to_string()), "{names:?}");
    }
    assert!(!names.contains(&"undo".to_string()), "{names:?}");
}

#[test]
fn help_describes_lifecycle_commands() {
    let out = Command::new(dispatcher_bin())
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ready"));
    assert!(stdout.contains("set"));
    assert!(stdout.contains("go"));
    assert!(stdout.contains("capability lifecycle"));
    let stale_bootstrap = "Bootstrap".to_string() + " a Rust workspace";
    assert!(!stdout.contains(&stale_bootstrap));
    assert!(!stdout.contains("undo"));
}

#[test]
fn dispatcher_runs_via_lib_entry_point() {
    // Sanity: the library entry point is reachable from in-process callers,
    // returning a clean ExitCode for an unknown subcommand.
    let argv: Vec<OsString> = ["ready-set", "totallyfake"]
        .iter()
        .map(OsString::from)
        .collect();
    let code = ready_set::run(argv);
    assert_eq!(code.as_u8(), 127);
}
