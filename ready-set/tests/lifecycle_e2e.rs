//! End-to-end tests for provider-backed lifecycle commands.

#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;

#[cfg(unix)]
const fn dispatcher() -> &'static str {
    env!("CARGO_BIN_EXE_ready-set")
}

#[cfg(unix)]
fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[cfg(unix)]
fn make_fake_plugin(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("ready-set-fake");
    write(
        &bin,
        r#"#!/bin/sh
if [ -n "$FAKE_LIFECYCLE_LOG" ]; then
  printf '%s %s\n' "$1" "$2" >> "$FAKE_LIFECYCLE_LOG"
fi
if [ "$1" = "__ready" ]; then
  if [ "$FAKE_READY_INVALID_JSON" = "1" ]; then
    printf 'not json\n'
    exit 0
  fi
  printf '{"id":"formatting","title":"Formatting","provider":"fake","state":"ready","relevance":"required","summary":"fake ready","next_action":null}\n'
  exit 0
fi
if [ "$1" = "__set" ]; then
  if [ "$FAKE_SET_FAIL" = "1" ]; then
    printf 'fake set failed\n' >&2
    exit 1
  fi
  if [ "$READY_SET_OUTPUT" = "json" ]; then
    if [ "$FAKE_SET_INVALID_JSON" = "1" ]; then
      printf 'not json\n'
      exit 0
    fi
    printf '{"id":"formatting","verb":"set","status":"changed","actions":[{"kind":"check","summary":"fake set"}]}\n'
  else
    printf 'fake set %s\n' "$2"
  fi
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    write(
        &dir.join("ready-set-fake.toml"),
        r#"
description              = "Fake provider"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false

[[capabilities]]
id = "formatting"
title = "Formatting"
provider = "fake"
verbs = ["ready", "set"]
default_relevance = "required"
"#,
    );
    bin
}

#[cfg(unix)]
fn fake_path() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    drop(make_fake_plugin(dir.path()));
    dir
}

#[cfg(unix)]
fn make_fake_go_plugin(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("ready-set-go-fake");
    write(
        &bin,
        r#"#!/bin/sh
if [ "$1" = "__go" ]; then
  if [ -n "$FAKE_GO_LOG" ]; then
    printf '%s\n' "$2" >> "$FAKE_GO_LOG"
  fi
  if [ "$FAKE_GO_INVALID_JSON" = "1" ]; then
    printf 'not json\n'
    exit 1
  fi
  if [ "$2" = "formatting" ] && [ "$FAKE_GO_FAIL_FORMATTING" = "1" ]; then
    printf '{"id":"formatting","verb":"go","status":"failed","actions":[{"kind":"error","summary":"fake go failed"}]}\n'
    exit 1
  fi
  printf '{"id":"%s","verb":"go","status":"ok","actions":[{"kind":"run","summary":"fake go %s"}]}\n' "$2" "$2"
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    write(
        &dir.join("ready-set-go-fake.toml"),
        r#"
description              = "Fake go provider"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false

[[capabilities]]
id = "formatting"
title = "Formatting"
provider = "go-fake"
verbs = ["ready", "set", "go"]
default_relevance = "required"

[[capabilities]]
id = "linting"
title = "Linting"
provider = "go-fake"
verbs = ["ready", "set", "go"]
default_relevance = "required"
"#,
    );
    bin
}

#[cfg(unix)]
fn fake_go_path() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    drop(make_fake_go_plugin(dir.path()));
    dir
}

#[cfg(unix)]
fn make_fake_set_only_plugin(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("ready-set-set-only");
    write(
        &bin,
        r#"#!/bin/sh
if [ -n "$FAKE_LIFECYCLE_LOG" ]; then
  printf '%s %s\n' "$1" "$2" >> "$FAKE_LIFECYCLE_LOG"
fi
if [ "$1" = "__set" ]; then
  printf '{"id":"setup-only","verb":"set","status":"ok","actions":[{"kind":"check","summary":"set only"}]}\n'
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    write(
        &dir.join("ready-set-set-only.toml"),
        r#"
description              = "Set-only provider"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false

[[capabilities]]
id = "setup-only"
title = "Setup Only"
provider = "set-only"
verbs = ["set"]
default_relevance = "required"
"#,
    );
    bin
}

#[cfg(unix)]
fn fake_set_only_path() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    drop(make_fake_set_only_plugin(dir.path()));
    dir
}

#[cfg(unix)]
fn make_fake_describe_plugin(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("ready-set-described");
    write(
        &bin,
        r#"#!/bin/sh
if [ "$1" = "__describe" ]; then
  printf '{"description":"Describe provider","version":"0.1.0","stability":"stable","min_dispatcher_version":"0.1.0","platforms":["linux","macos","windows"],"requires_cargo_workspace":false,"capabilities":[{"id":"described","title":"Described","provider":"described","verbs":["ready"],"default_relevance":"required"}]}\n'
  exit 0
fi
if [ "$1" = "__ready" ]; then
  printf '{"id":"described","title":"Described","provider":"described","state":"ready","relevance":"required","summary":"described ready","next_action":null}\n'
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    bin
}

#[cfg(unix)]
fn fake_describe_path() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    drop(make_fake_describe_plugin(dir.path()));
    dir
}

#[cfg(unix)]
fn make_fake_env_plugin(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin = dir.join("ready-set-envcheck");
    write(
        &bin,
        r#"#!/bin/sh
if [ "$1" != "__ready" ] && [ -n "$FAKE_ENV_LOG" ]; then
  printf '%s %s\n' "$1" "$2" > "$FAKE_ENV_LOG"
fi
if [ "$1" = "__ready" ]; then
  {
    printf 'OUTPUT=%s\n' "$READY_SET_OUTPUT"
    printf 'LOG=%s\n' "$READY_SET_LOG"
    printf 'COLOR=%s\n' "$READY_SET_COLOR"
    printf 'ROOT=%s\n' "$READY_SET_PROJECT_ROOT"
    printf 'CONFIG=%s\n' "$READY_SET_CONFIG_PATH"
    printf 'UNKNOWN=%s\n' "${READY_SET_E2E_UNKNOWN-unset}"
  } > "$FAKE_ENV_LOG"
  printf '{"id":"envcheck","title":"Env Check","provider":"envcheck","state":"ready","relevance":"required","summary":"env ok","next_action":null}\n'
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&bin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bin, perms).unwrap();
    write(
        &dir.join("ready-set-envcheck.toml"),
        r#"
description              = "Env provider"
version                  = "0.1.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false

[[capabilities]]
id = "envcheck"
title = "Env Check"
provider = "envcheck"
verbs = ["ready"]
default_relevance = "required"
"#,
    );
    bin
}

#[cfg(unix)]
fn fake_env_path() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    drop(make_fake_env_plugin(dir.path()));
    dir
}

#[cfg(unix)]
#[test]
fn bare_command_prints_same_matrix_as_ready() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let bare = Command::new(dispatcher())
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();
    let ready = Command::new(dispatcher())
        .arg("ready")
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(bare.status.success());
    assert!(ready.status.success());
    assert_eq!(
        String::from_utf8(bare.stdout).unwrap(),
        String::from_utf8(ready.stdout).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn ready_json_invokes_provider_ready() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "ready"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityReport> = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id.as_str(), "formatting");
    assert_eq!(parsed[0].state, ready_set_sdk::CapabilityState::Ready);
}

#[cfg(unix)]
#[test]
fn set_json_invokes_provider_set() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "set", "formatting"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityRunReport> =
        serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id.as_str(), "formatting");
}

#[cfg(unix)]
#[test]
fn go_json_invokes_provider_go() {
    let bin_dir = fake_go_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "go", "formatting"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityRunReport> =
        serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id.as_str(), "formatting");
    assert_eq!(parsed[0].verb, ready_set_sdk::CapabilityVerb::Go);
    assert_eq!(parsed[0].status, ready_set_sdk::RunStatus::Ok);
}

#[cfg(unix)]
#[test]
fn go_local_json_flag_invokes_provider_go() {
    let bin_dir = fake_go_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["go", "--json", "formatting"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityRunReport> =
        serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id.as_str(), "formatting");
}

#[cfg(unix)]
#[test]
fn go_rejects_unsupported_verb_before_spawning_provider() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["go", "formatting"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("capability `formatting` does not support go"));
}

#[cfg(unix)]
#[test]
fn go_json_continues_after_failed_report() {
    let bin_dir = fake_go_path();
    let work = tempfile::tempdir().unwrap();
    let log = work.path().join("go.log");
    let out = Command::new(dispatcher())
        .args(["--json", "go"])
        .env("PATH", bin_dir.path())
        .env("FAKE_GO_FAIL_FORMATTING", "1")
        .env("FAKE_GO_LOG", &log)
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityRunReport> =
        serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].id.as_str(), "formatting");
    assert_eq!(parsed[0].status, ready_set_sdk::RunStatus::Failed);
    assert_eq!(parsed[1].id.as_str(), "linting");
    assert_eq!(parsed[1].status, ready_set_sdk::RunStatus::Ok);
    assert_eq!(
        std::fs::read_to_string(log).unwrap(),
        "formatting\nlinting\n"
    );
}

#[cfg(unix)]
#[test]
fn describe_metadata_capability_appears_in_ready_matrix() {
    let bin_dir = fake_describe_path();
    let work = tempfile::tempdir().unwrap();
    let warmup = Command::new(bin_dir.path().join("ready-set-described"))
        .arg("__describe")
        .output()
        .unwrap();
    assert!(warmup.status.success());
    let out = Command::new(dispatcher())
        .args(["--json", "ready", "described"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityReport> = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].id.as_str(), "described");
    assert_eq!(parsed[0].state, ready_set_sdk::CapabilityState::Ready);
}

#[cfg(unix)]
#[test]
fn ready_rejects_unsupported_verb_before_spawning_provider() {
    let bin_dir = fake_set_only_path();
    let work = tempfile::tempdir().unwrap();
    let log = work.path().join("lifecycle.log");
    let out = Command::new(dispatcher())
        .args(["ready", "setup-only"])
        .env("PATH", bin_dir.path())
        .env("FAKE_LIFECYCLE_LOG", &log)
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!log.exists());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("capability `setup-only` does not support ready"));
}

#[cfg(unix)]
#[test]
fn set_rejects_unsupported_verb_before_spawning_provider() {
    let bin_dir = fake_env_path();
    let work = tempfile::tempdir().unwrap();
    let log = work.path().join("lifecycle.log");
    let out = Command::new(dispatcher())
        .args(["set", "envcheck"])
        .env("PATH", bin_dir.path())
        .env("FAKE_ENV_LOG", &log)
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!log.exists());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("capability `envcheck` does not support set"));
}

#[cfg(unix)]
#[test]
fn go_rejects_unsupported_verb_without_spawning_provider() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let log = work.path().join("lifecycle.log");
    let out = Command::new(dispatcher())
        .args(["go", "formatting"])
        .env("PATH", bin_dir.path())
        .env("FAKE_LIFECYCLE_LOG", &log)
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(!log.exists());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("capability `formatting` does not support go"));
}

#[cfg(unix)]
#[test]
fn set_provider_failure_propagates_nonzero() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["set", "formatting"])
        .env("PATH", bin_dir.path())
        .env("FAKE_SET_FAIL", "1")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("fake set failed"));
}

#[cfg(unix)]
#[test]
fn set_json_invalid_output_is_contract_violation() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "set", "formatting"])
        .env("PATH", bin_dir.path())
        .env("FAKE_SET_INVALID_JSON", "1")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(5));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("invalid set JSON"));
}

#[cfg(unix)]
#[test]
fn go_json_invalid_output_is_contract_violation() {
    let bin_dir = fake_go_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "go", "formatting"])
        .env("PATH", bin_dir.path())
        .env("FAKE_GO_INVALID_JSON", "1")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(5));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("invalid go JSON"));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityRunReport> =
        serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].status, ready_set_sdk::RunStatus::Failed);
}

#[cfg(unix)]
#[test]
fn ready_json_invalid_output_becomes_blocked_report() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["--json", "ready", "formatting"])
        .env("PATH", bin_dir.path())
        .env("FAKE_READY_INVALID_JSON", "1")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: Vec<ready_set_sdk::CapabilityReport> = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].state, ready_set_sdk::CapabilityState::Blocked);
    assert!(parsed[0].summary.contains("invalid readiness JSON"));
}

#[cfg(unix)]
#[test]
fn lifecycle_invocation_exports_env_contract_and_strips_unknown_ready_set_vars() {
    let bin_dir = fake_env_path();
    let work = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(work.path().join(".git")).unwrap();
    write(
        &work.path().join(".ready-set.toml"),
        "[ready-set]\nschema_version = 2\nprofile = \"test\"\n",
    );
    let log = work.path().join("env.log");
    let out = Command::new(dispatcher())
        .args([
            "--json",
            "--verbose",
            "--color",
            "never",
            "ready",
            "envcheck",
        ])
        .env("PATH", bin_dir.path())
        .env("FAKE_ENV_LOG", &log)
        .env("READY_SET_E2E_UNKNOWN", "should-not-leak")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let env_log = std::fs::read_to_string(log).unwrap();
    assert!(env_log.contains("OUTPUT=json\n"));
    assert!(env_log.contains("LOG=verbose\n"));
    assert!(env_log.contains("COLOR=never\n"));
    assert!(env_log.contains(&format!(
        "ROOT={}\n",
        std::fs::canonicalize(work.path()).unwrap().display()
    )));
    assert!(env_log.contains(&format!(
        "CONFIG={}\n",
        std::fs::canonicalize(work.path().join(".ready-set.toml"))
            .unwrap()
            .display()
    )));
    assert!(env_log.contains("UNKNOWN=unset\n"));
}

#[cfg(unix)]
#[test]
fn unknown_capability_exits_user_error() {
    let bin_dir = fake_path();
    let work = tempfile::tempdir().unwrap();
    let out = Command::new(dispatcher())
        .args(["ready", "unknown"])
        .env("PATH", bin_dir.path())
        .current_dir(work.path())
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown capability `unknown`"));
}

#[cfg(unix)]
#[test]
fn ready_without_plugins_creates_no_files() {
    let work = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(work.path().join(".git")).unwrap();

    let out = Command::new(dispatcher())
        .arg("ready")
        .env("PATH", "")
        .current_dir(work.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(!work.path().join(".ready-set").exists());
    assert!(!work.path().join(".ready-set.toml").exists());
}
