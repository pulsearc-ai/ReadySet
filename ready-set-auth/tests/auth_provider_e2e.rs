//! End-to-end tests for the `ready-set-auth` provider plugin.

use std::path::Path;
use std::process::Command;

const fn plugin() -> &'static str {
    env!("CARGO_BIN_EXE_ready-set-auth")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn unwired_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("backend/Cargo.toml"),
        "[package]\nname = \"example-api\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\naxum = \"0.8\"\n",
    );
    write(
        &dir.path().join("backend/src/main.rs"),
        r#"const SESSION_COOKIE: &str = "app_session";
fn session_cookie() {}
fn sign_claims() {}
fn main() {
    let _ = SESSION_COOKIE;
}
"#,
    );
    write(
        &dir.path().join("backend/src/migrations/0001_init.sql"),
        "CREATE TABLE users (email TEXT PRIMARY KEY);\n",
    );
    write(
        &dir.path().join("app/src/client/pages/Login.tsx"),
        r#"function SsoButton({ label }: { label: string }) {
  return <button type="button">{label}</button>;
}
<SsoButton label="Continue with Google" />
<SsoButton label="Continue with GitHub" />
throw new Error("Account creation is invite-only");
"#,
    );
    write(
        &dir.path().join(".env.example"),
        "APP_ORIGIN=http://localhost:3000\n",
    );
    write(
        &dir.path().join("backend/.env.example"),
        "SESSION_SECRET=\nAPP_ORIGIN=http://localhost:3000\n",
    );
    dir
}

fn wired_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join("backend/Cargo.toml"),
        r#"[package]
name = "example-api"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = "0.8"
"#,
    );
    write(
        &dir.path().join("backend/src/main.rs"),
        r#"const SESSION_COOKIE: &str = "app_session";
fn session_cookie() {}
fn sign_claims() {}
fn oauth_start() {}
fn oauth_callback() {}
fn main() {
    let _ = "/api/auth/oauth/google/start";
    let _ = "/api/auth/oauth/github/callback";
}
"#,
    );
    write(
        &dir.path().join("backend/src/migrations/0002_oauth.sql"),
        "CREATE TABLE oauth_identities (provider TEXT NOT NULL, provider_subject TEXT NOT NULL, email TEXT NOT NULL);\n",
    );
    write(
        &dir.path().join("app/src/client/pages/Login.tsx"),
        r#"function SsoButton({ provider, label }: { provider: string; label: string }) {
  return <button type="button" onClick={() => window.location.assign(`/api/auth/oauth/${provider}/start`)}>{label}</button>;
}
<SsoButton provider="google" label="Continue with Google" />
<SsoButton provider="github" label="Continue with GitHub" />
throw new Error("Account creation is invite-only");
"#,
    );
    let oauth_env = r"OAUTH_GOOGLE_CLIENT_ID=
OAUTH_GOOGLE_CLIENT_SECRET=
OAUTH_GITHUB_CLIENT_ID=
OAUTH_GITHUB_CLIENT_SECRET=
";
    write(&dir.path().join(".env.example"), oauth_env);
    write(&dir.path().join("backend/.env.example"), oauth_env);
    dir
}

fn configured_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        &dir.path().join(".ready-set/plugins/auth/config.toml"),
        r#"
recognize_paths = ["service/app.py"]
server_sources = ["service/app.py"]
route_markers = ["oauth_start", "oauth_callback"]
session_markers = ["issue_session"]
identity_sources = ["service/models.py"]
identity_markers = ["oauth_accounts"]
env_examples = ["config/example.env"]
env_vars = ["AUTH0_CLIENT_ID", "AUTH0_CLIENT_SECRET"]
client_sources = ["web/login.html"]
login_markers = ["/oauth/start"]
account_policy_markers = ["invite required"]
local_source_paths = ["vendor/ready-set-auth"]
"#,
    );
    write(&dir.path().join("pyproject.toml"), "[project]\n");
    write(
        &dir.path().join("service/app.py"),
        "def oauth_start(): pass\ndef oauth_callback(): pass\ndef issue_session(): pass\n",
    );
    write(
        &dir.path().join("service/models.py"),
        "oauth_accounts = []\n",
    );
    write(
        &dir.path().join("config/example.env"),
        "AUTH0_CLIENT_ID=\nAUTH0_CLIENT_SECRET=\n",
    );
    write(
        &dir.path().join("web/login.html"),
        r#"<a href="/oauth/start">Sign in</a><!-- invite required -->"#,
    );
    write(&dir.path().join("vendor/ready-set-auth"), "");
    dir
}

#[test]
fn describe_emits_auth_capability() {
    let out = Command::new(plugin()).arg("__describe").output().unwrap();

    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let capabilities = parsed["capabilities"].as_array().unwrap();
    assert_eq!(capabilities.len(), 1);
    assert_eq!(capabilities[0]["id"], "auth");
    assert_eq!(capabilities[0]["provider"], "auth");
    assert!(parsed.get("project_requirements").is_none());
}

#[test]
fn ready_reports_incomplete_for_unwired_project() {
    let dir = unwired_project();
    let out = Command::new(plugin())
        .args(["__ready", "auth"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.id.as_str(), "auth");
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Incomplete);
    assert_eq!(
        report
            .next_action
            .as_ref()
            .map(|next| next.command.as_str()),
        Some("ready-set set auth")
    );
}

#[test]
fn ready_reports_ready_for_wired_project() {
    let dir = wired_project();
    let out = Command::new(plugin())
        .args(["__ready", "auth"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Ready);
    assert!(report.next_action.is_none());
}

#[test]
fn ready_uses_custom_auth_config_for_different_layout() {
    let dir = configured_project();
    let out = Command::new(plugin())
        .args(["__ready", "auth"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Ready);
    assert!(report.next_action.is_none());
}

#[test]
fn set_writes_plan_and_change_log() {
    let dir = unwired_project();
    let out = Command::new(plugin())
        .args(["__set", "auth"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        dir.path()
            .join(".ready-set/plugins/auth/implementation-plan.md")
            .is_file()
    );
    let plan = std::fs::read_to_string(
        dir.path()
            .join(".ready-set/plugins/auth/implementation-plan.md"),
    )
    .unwrap();
    assert!(plan.contains("# Auth Integration Plan"));
    assert!(plan.contains("session or account bridge"));
    assert!(!plan.contains("app_session"));
    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Changed);
    assert!(dir.path().join(".ready-set/changes").is_dir());
}

#[test]
fn set_writes_generic_plan_without_recognized_layout() {
    let dir = tempfile::tempdir().unwrap();
    let out = Command::new(plugin())
        .args(["__set", "auth"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plan = std::fs::read_to_string(
        dir.path()
            .join(".ready-set/plugins/auth/implementation-plan.md"),
    )
    .unwrap();
    assert!(plan.contains("No existing web-auth surface was detected"));
    assert!(plan.contains("provider-neutral OAuth/OIDC integration template"));
    assert!(plan.contains("Choose the application boundary"));
    assert!(!plan.contains("backend/Cargo.toml"));
}

#[test]
fn dry_run_writes_nothing() {
    let dir = unwired_project();
    let out = Command::new(plugin())
        .args(["__set", "auth", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(out.status.success());
    assert!(!dir.path().join(".ready-set").exists());
}

#[test]
fn go_json_fails_when_required_checks_are_missing() {
    let dir = unwired_project();
    let out = Command::new(plugin())
        .args(["__go", "auth"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Failed);
    assert!(report.actions.iter().any(|action| {
        action.kind == ready_set_sdk::CapabilityActionKind::Error
            && action.summary.contains("OAuth start/callback routes")
    }));
}

#[test]
fn go_uses_custom_auth_config_for_different_layout() {
    let dir = configured_project();
    let out = Command::new(plugin())
        .args(["__go", "auth"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Ok);
    assert!(report.actions.iter().any(|action| {
        action.kind == ready_set_sdk::CapabilityActionKind::Check
            && action.path.as_deref() == Some("service/app.py")
            && action.summary.contains("session or account bridge")
    }));
}

#[test]
fn direct_invocation_runs_auth_audit() {
    let dir = wired_project();
    let out = Command::new(plugin())
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("ready-set-auth go auth"));
}
