//! End-to-end tests for the `ready-set-encrypt` provider plugin.

use std::path::{Path, PathBuf};
use std::process::Command;

use ready_set_sdk::change_log::{ChangeOp, reverse_dir};

const fn plugin() -> &'static str {
    env!("CARGO_BIN_EXE_ready-set-encrypt")
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn fresh_project() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn project_with_refs() -> tempfile::TempDir {
    let dir = fresh_project();
    write(
        &dir.path().join("src/main.rs"),
        r#"fn main() {
    let _ = std::env::var("DATABASE_URL").unwrap();
    let _ = std::env::var("API_KEY").unwrap();
}
"#,
    );
    write(
        &dir.path().join("web/app.ts"),
        "export const url = process.env.NEXT_PUBLIC_URL;\nexport const v = import.meta.env.VITE_API;\n",
    );
    dir
}

#[test]
fn describe_emits_secrets_and_rotation_capabilities() {
    let out = Command::new(plugin()).arg("__describe").output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let capabilities = parsed["capabilities"].as_array().unwrap();
    assert_eq!(capabilities.len(), 3);

    let ids: Vec<&str> = capabilities
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"secrets"));
    assert!(ids.contains(&"rotation"));
    assert!(ids.contains(&"secret-bundles"));
    for capability in capabilities {
        assert_eq!(capability["provider"], "encrypt");
    }

    let verbs = |id: &str| -> Vec<String> {
        capabilities.iter().find(|c| c["id"] == id).unwrap()["verbs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    };
    let secrets_verbs = verbs("secrets");
    assert!(secrets_verbs.contains(&"ready".to_owned()));
    assert!(secrets_verbs.contains(&"set".to_owned()));
    assert!(secrets_verbs.contains(&"go".to_owned()));

    let rotation_verbs = verbs("rotation");
    assert!(rotation_verbs.contains(&"ready".to_owned()));
    assert!(rotation_verbs.contains(&"go".to_owned()));
    assert!(!rotation_verbs.contains(&"set".to_owned()));

    let bundle_verbs = verbs("secret-bundles");
    assert!(bundle_verbs.contains(&"ready".to_owned()));
    assert!(bundle_verbs.contains(&"set".to_owned()));
    assert!(bundle_verbs.contains(&"go".to_owned()));

    assert!(parsed.get("project_requirements").is_none());

    let aliases = parsed["command_aliases"].as_array().unwrap();
    assert!(aliases.iter().any(|alias| {
        alias["name"] == "rotate" && alias["target"] == "go" && alias["capability"] == "rotation"
    }));
    assert!(aliases.iter().any(|alias| {
        alias["name"] == "encrypt"
            && alias["target"] == "set"
            && alias["capability"] == "secret-bundles"
    }));
    assert!(aliases.iter().any(|alias| {
        alias["name"] == "encrypt"
            && alias["target"] == "plugin"
            && alias["match_first_arg"] == "bundle"
    }));
    assert!(aliases.iter().any(|alias| {
        alias["name"] == "encrypt"
            && alias["target"] == "plugin"
            && alias["match_first_arg"] == "exec"
    }));
}

#[test]
#[allow(clippy::too_many_lines)]
fn secret_bundles_set_and_go_round_trip_configured_dotenv() {
    let dir = fresh_project();
    write(
        &dir.path().join(".env"),
        "API_KEY=super-secret\nAPP_ENV=test\nEMPTY_QUOTED=''\n",
    );
    write(
        &dir.path().join(".ready-set/plugins/secrets/config.toml"),
        r#"[bundles]
enabled = true
key_file = "secrets/readyset-bundle.key"

[[bundles.files]]
source = ".env"
encrypted = "deploy/secrets/root.env.rsb"
payload = "dotenv"
environment = "test"
redact_source = true
"#,
    );

    let set = Command::new(plugin())
        .args(["__set", "secret-bundles"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(dir.path().join("secrets/readyset-bundle.key").is_file());
    let bundle_path = dir.path().join("deploy/secrets/root.env.rsb");
    assert!(bundle_path.is_file());
    let bundle_raw = std::fs::read_to_string(&bundle_path).unwrap();
    assert!(!bundle_raw.contains("super-secret"));
    let source_after_set = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert_eq!(source_after_set, "API_KEY=\nAPP_ENV=\nEMPTY_QUOTED=\n");
    assert!(!source_after_set.contains("super-secret"));

    let second_set = Command::new(plugin())
        .args(["__set", "secret-bundles"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        second_set.status.success(),
        "{}",
        String::from_utf8_lossy(&second_set.stderr)
    );
    let second_report: serde_json::Value = serde_json::from_slice(
        String::from_utf8(second_set.stdout)
            .unwrap()
            .trim()
            .as_bytes(),
    )
    .unwrap();
    assert_eq!(second_report["status"], "noop");

    let ready = Command::new(plugin())
        .args(["__ready", "secret-bundles"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(ready.status.success());
    let ready_json: serde_json::Value =
        serde_json::from_slice(String::from_utf8(ready.stdout).unwrap().trim().as_bytes()).unwrap();
    assert_eq!(ready_json["state"], "ready");

    let go = Command::new(plugin())
        .args(["__go", "secret-bundles"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        go.status.success(),
        "{}",
        String::from_utf8_lossy(&go.stderr)
    );
    let report: serde_json::Value =
        serde_json::from_slice(String::from_utf8(go.stdout).unwrap().trim().as_bytes()).unwrap();
    assert_eq!(report["status"], "ok");

    let status = Command::new(plugin())
        .args(["bundle", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let stdout = String::from_utf8(status.stdout).unwrap();
    assert!(stdout.contains("keys: 3 (API_KEY, APP_ENV, EMPTY_QUOTED)"));
    assert!(stdout.contains("non-empty: 2 (API_KEY, APP_ENV)"));
    assert!(stdout.contains("drift: source redacted; bundle clean"));
    assert!(!stdout.contains("super-secret"));

    write(
        &dir.path().join(".env"),
        "API_KEY=super-secret\nAPP_ENV=test\nOAUTH_GOOGLE_CLIENT_SECRET=plain-secret\n",
    );
    let status = Command::new(plugin())
        .args(["bundle", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    let stdout = String::from_utf8(status.stdout).unwrap();
    assert!(stdout.contains("drift: added: OAUTH_GOOGLE_CLIENT_SECRET"));
    assert!(stdout.contains("plaintext exposed: API_KEY, APP_ENV, OAUTH_GOOGLE_CLIENT_SECRET"));
    assert!(!stdout.contains("plain-secret"));

    let stale = Command::new(plugin())
        .args(["__go", "secret-bundles"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!stale.status.success());
    let stdout = String::from_utf8(stale.stdout).unwrap();
    let stale_report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(stale_report["status"], "failed");
    let rendered = stale_report.to_string();
    assert!(rendered.contains("plaintext values not captured"));
    assert!(!rendered.contains("plain-secret"));

    let reconcile = Command::new(plugin())
        .args(["__set", "secret-bundles"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        reconcile.status.success(),
        "{}",
        String::from_utf8_lossy(&reconcile.stderr)
    );
    let stdout = String::from_utf8(reconcile.stdout).unwrap();
    let reconcile_report: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let rendered = reconcile_report.to_string();
    assert!(rendered.contains("added: OAUTH_GOOGLE_CLIENT_SECRET"));
    assert!(!rendered.contains("plain-secret"));
    let source_after_reconcile = std::fs::read_to_string(dir.path().join(".env")).unwrap();
    assert_eq!(
        source_after_reconcile,
        "API_KEY=\nAPP_ENV=\nOAUTH_GOOGLE_CLIENT_SECRET=\n"
    );

    let status = Command::new(plugin())
        .args(["bundle", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    let stdout = String::from_utf8(status.stdout).unwrap();
    assert!(stdout.contains("keys: 3 (API_KEY, APP_ENV, OAUTH_GOOGLE_CLIENT_SECRET)"));
    assert!(stdout.contains("drift: source redacted; bundle clean"));
    assert!(!stdout.contains("plain-secret"));
}

#[test]
#[allow(clippy::too_many_lines)]
fn configured_exec_filters_environment_and_names() {
    let dir = fresh_project();
    write(
        &dir.path().join(".env.local"),
        "API_KEY=local-secret\nSHARED=local-shared\nSKIP_LOCAL=nope\n",
    );
    write(
        &dir.path().join(".env.prod"),
        "API_KEY=prod-secret\nPROD_ONLY=prod-only\n",
    );
    write(
        &dir.path().join(".ready-set/plugins/secrets/config.toml"),
        r#"[bundles]
enabled = true
key_file = "secrets/readyset-bundle.key"

[bundles.runtime]
default_environment = "local"
include_names = ["API_KEY", "SHARED", "PROD_ONLY"]
exclude_names = ["SHARED"]

[[bundles.files]]
source = ".env.local"
encrypted = "deploy/secrets/local.env.rsb"
payload = "dotenv"
environment = "local"
redact_source = true
exclude_names = ["SKIP_LOCAL"]

[[bundles.files]]
source = ".env.prod"
encrypted = "deploy/secrets/prod.env.rsb"
payload = "dotenv"
environment = "prod"
redact_source = true
"#,
    );

    let set = Command::new(plugin())
        .args(["__set", "secret-bundles"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );

    let local = Command::new(plugin())
        .args([
            "exec",
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s|%s' \"$API_KEY\" \"${SHARED-unset}\" \"${PROD_ONLY-unset}\" \"${SKIP_LOCAL-unset}\"",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        local.status.success(),
        "{}",
        String::from_utf8_lossy(&local.stderr)
    );
    assert_eq!(
        String::from_utf8(local.stdout).unwrap(),
        "local-secret|unset|unset|unset"
    );

    let prod = Command::new(plugin())
        .args([
            "exec",
            "--env",
            "prod",
            "--",
            "sh",
            "-c",
            "printf '%s|%s|%s' \"$API_KEY\" \"${PROD_ONLY-unset}\" \"${SHARED-unset}\"",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        prod.status.success(),
        "{}",
        String::from_utf8_lossy(&prod.stderr)
    );
    assert_eq!(
        String::from_utf8(prod.stdout).unwrap(),
        "prod-secret|prod-only|unset"
    );

    let cli_include = Command::new(plugin())
        .args([
            "exec",
            "--env",
            "prod",
            "--include",
            "PROD_ONLY",
            "--",
            "sh",
            "-c",
            "printf '%s|%s' \"${API_KEY-unset}\" \"$PROD_ONLY\"",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        cli_include.status.success(),
        "{}",
        String::from_utf8_lossy(&cli_include.stderr)
    );
    assert_eq!(
        String::from_utf8(cli_include.stdout).unwrap(),
        "unset|prod-only"
    );
}

#[test]
fn secret_bundles_can_use_one_time_env_key_without_saving_key_file() {
    let dir = fresh_project();
    write(&dir.path().join(".env"), "API_KEY=super-secret\n");
    write(
        &dir.path().join(".ready-set/plugins/secrets/config.toml"),
        r#"[bundles]
enabled = true
key_env = "READYSET_BUNDLE_KEY"

[[bundles.files]]
source = ".env"
encrypted = "deploy/secrets/root.env.rsb"
payload = "dotenv"
environment = "test"
redact_source = true
"#,
    );

    let generated = Command::new(plugin())
        .args(["key", "generate"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let stdout = String::from_utf8(generated.stdout).unwrap();
    assert!(stdout.contains("ReadySet did not save it"));
    let token = stdout
        .lines()
        .find_map(|line| line.strip_prefix("READYSET_BUNDLE_KEY="))
        .unwrap()
        .to_owned();

    let set = Command::new(plugin())
        .args(["__set", "secret-bundles"])
        .env("READYSET_BUNDLE_KEY", &token)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert!(!dir.path().join("secrets/readyset-bundle.key").exists());
    assert!(dir.path().join("deploy/secrets/root.env.rsb").is_file());
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".env")).unwrap(),
        "API_KEY=\n"
    );

    let go = Command::new(plugin())
        .args(["__go", "secret-bundles"])
        .env("READYSET_BUNDLE_KEY", &token)
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        go.status.success(),
        "{}",
        String::from_utf8_lossy(&go.stderr)
    );
}

#[test]
fn ready_reports_not_needed_when_empty_project() {
    let dir = fresh_project();
    let out = Command::new(plugin())
        .args(["__ready", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.id.as_str(), "secrets");
    assert_eq!(report.state, ready_set_sdk::CapabilityState::NotNeeded);
}

#[test]
fn ready_reports_missing_when_no_example_but_refs_exist() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__ready", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Missing);
    assert!(report.next_action.is_some());
}

#[test]
fn ready_reports_incomplete_when_example_missing_refs() {
    let dir = project_with_refs();
    write(&dir.path().join(".env.example"), "DATABASE_URL=\n");
    let out = Command::new(plugin())
        .args(["__ready", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Incomplete);
    assert!(report.summary.contains("API_KEY") || report.summary.contains("NEXT_PUBLIC_URL"));
}

#[test]
fn set_creates_env_example_gitignore_and_gitleaks() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let env_example = std::fs::read_to_string(dir.path().join(".env.example")).unwrap();
    assert!(env_example.contains("DATABASE_URL="));
    assert!(env_example.contains("API_KEY="));
    assert!(env_example.contains("NEXT_PUBLIC_URL="));
    assert!(env_example.contains("VITE_API="));

    let gitignore = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gitignore.contains("# >>> ready-set-encrypt managed >>>"));
    assert!(gitignore.contains(".env\n"));
    assert!(gitignore.contains("secrets/\n"));

    assert!(dir.path().join(".gitleaks.toml").is_file());

    let stdout = String::from_utf8(out.stdout).unwrap();
    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Changed);
}

#[test]
fn set_is_idempotent() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let env_before = std::fs::read_to_string(dir.path().join(".env.example")).unwrap();
    let gi_before = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    let leaks_before = std::fs::read_to_string(dir.path().join(".gitleaks.toml")).unwrap();

    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    assert_eq!(
        std::fs::read_to_string(dir.path().join(".env.example")).unwrap(),
        env_before
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitignore")).unwrap(),
        gi_before
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".gitleaks.toml")).unwrap(),
        leaks_before
    );

    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Noop);
}

#[test]
fn dry_run_writes_nothing() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__set", "secrets", "--dry-run"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!dir.path().join(".env.example").exists());
    assert!(!dir.path().join(".gitignore").exists());
    assert!(!dir.path().join(".gitleaks.toml").exists());
    assert!(!dir.path().join(".ready-set").exists());
}

#[test]
fn changelog_uses_encrypt_provider_name() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let entries: Vec<_> = std::fs::read_dir(dir.path().join(".ready-set/changes"))
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(entries.len(), 1);
    let name = entries[0].file_name();
    assert!(name.to_string_lossy().starts_with("encrypt-"));
}

#[test]
fn set_changelog_can_be_reversed_to_clean_tree() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let records = reverse_dir(dir.path()).unwrap();
    assert!(!records.is_empty());

    for (_, record) in records {
        let path = dir.path().join(&record.path);
        match record.op {
            ChangeOp::Create => std::fs::remove_file(path).unwrap(),
            ChangeOp::Modify => {
                let before_sha = record.before_sha256.as_ref().unwrap();
                let backup = dir.path().join(".ready-set/backups").join(before_sha);
                std::fs::copy(backup, path).unwrap();
            },
            ChangeOp::Delete => unreachable!("set secrets does not delete files"),
        }
    }

    assert!(!dir.path().join(".env.example").exists());
    assert!(!dir.path().join(".gitignore").exists());
    assert!(!dir.path().join(".gitleaks.toml").exists());
}

#[test]
fn set_coexists_with_other_managed_gitignore_block() {
    let dir = project_with_refs();
    let rust_block =
        "# >>> ready-set managed >>>\ntarget/\nCargo.lock\n# <<< ready-set managed <<<\n";
    write(&dir.path().join(".gitignore"), rust_block);

    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let gi = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gi.contains("# >>> ready-set managed >>>"));
    assert!(gi.contains("target/\n"));
    assert!(gi.contains("# >>> ready-set-encrypt managed >>>"));
    assert!(gi.contains(".env\n"));
}

#[test]
fn go_secrets_returns_ok_on_clean_tree() {
    let dir = fresh_project();
    write(&dir.path().join("README.md"), "hello world\n");
    let out = Command::new(plugin())
        .args(["__go", "secrets"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Ok);
}

#[test]
fn go_secrets_fails_on_planted_leak_without_revealing_bytes() {
    let dir = fresh_project();
    let leak = "sk-ant-api03-".to_owned() + &"a".repeat(64);
    write(&dir.path().join("oops.rs"), &leak);

    // Hide gitleaks if it happens to be installed locally, so the regex
    // fallback runs deterministically.
    let empty_path: PathBuf = PathBuf::from("/var/empty");
    let out = Command::new(plugin())
        .args(["__go", "secrets"])
        .env("READY_SET_OUTPUT", "json")
        .env("PATH", &empty_path)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        !stdout.contains("sk-ant-api03-aaa"),
        "leak bytes leaked into output"
    );

    let report: ready_set_sdk::CapabilityRunReport = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Failed);
    let errors: Vec<_> = report
        .actions
        .iter()
        .filter(|a| a.kind == ready_set_sdk::CapabilityActionKind::Error)
        .collect();
    assert!(!errors.is_empty());
    for action in errors {
        assert!(!action.summary.contains("sk-ant"));
    }
}

#[test]
fn unknown_capability_is_rejected_for_go() {
    let dir = fresh_project();
    let out = Command::new(plugin())
        .args(["__go", "bogus"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(out.status.code(), Some(1));
}

// ---------------------------------------------------------------------------
// rotation capability (v0.2)
// ---------------------------------------------------------------------------

fn manifest_path(root: &Path) -> PathBuf {
    root.join(".ready-set/plugins/secrets/manifest.toml")
}

fn audit_log_path(root: &Path) -> PathBuf {
    root.join(".ready-set/plugins/secrets/rotations.jsonl")
}

#[test]
fn set_secrets_scaffolds_rotation_manifest() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let manifest = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    assert!(manifest.contains("[ready-set-encrypt]"));
    assert!(manifest.contains("schema_version = 1"));
    assert!(manifest.contains("default_cadence_days = 90"));
    assert!(manifest.contains("[secret.DATABASE_URL]"));
    assert!(manifest.contains("[secret.API_KEY]"));
    assert!(manifest.contains("backend = \"manual\""));
}

#[test]
fn set_secrets_additive_reconcile_preserves_user_edits() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // User customizes the manifest: switches FOO to self-issued, adds notes.
    let mut current = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    current = current.replace(
        "[secret.DATABASE_URL]\nbackend = \"manual\"",
        "[secret.DATABASE_URL]\nbackend = \"self-issued\"\ncadence_days = 30\nnotes = \"do not touch\"",
    );
    std::fs::write(manifest_path(dir.path()), &current).unwrap();

    // Add a new env var reference and re-run set.
    write(
        &dir.path().join("web/extra.ts"),
        "export const z = process.env.NEW_VAR;\n",
    );
    let out = Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let after = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    assert!(after.contains("notes = \"do not touch\""));
    assert!(after.contains("backend = \"self-issued\""));
    assert!(after.contains("[secret.NEW_VAR]"));
}

#[test]
fn ready_rotation_blocked_without_manifest() {
    let dir = project_with_refs();
    let out = Command::new(plugin())
        .args(["__ready", "rotation"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.id.as_str(), "rotation");
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Blocked);
    assert!(report.next_action.unwrap().command.contains("set secrets"));
}

#[test]
fn ready_rotation_stale_when_secret_never_rotated() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let out = Command::new(plugin())
        .args(["__ready", "rotation"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let report: ready_set_sdk::CapabilityReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.state, ready_set_sdk::CapabilityState::Stale);
    assert!(report.next_action.unwrap().command.contains("--confirm"));
}

#[test]
fn go_rotation_dry_run_writes_nothing() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!audit_log_path(dir.path()).exists());

    let out = Command::new(plugin())
        .args(["__go", "rotation"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(!audit_log_path(dir.path()).exists());
    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.status, ready_set_sdk::RunStatus::Noop);
    assert!(
        report
            .actions
            .iter()
            .any(|a| a.summary.contains("--confirm"))
    );
}

#[test]
fn go_rotation_confirm_writes_target_path_for_self_issued() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Convert one entry to self-issued with a target_path.
    let raw = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    let raw = raw.replace(
        "[secret.API_KEY]\nbackend = \"manual\"",
        "[secret.API_KEY]\nbackend = \"self-issued\"\ntarget_path = \"secrets/api-key\"",
    );
    std::fs::write(manifest_path(dir.path()), &raw).unwrap();

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let written = std::fs::read_to_string(dir.path().join("secrets/api-key")).unwrap();
    assert_eq!(written.trim().len(), 64);

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"name\":\"API_KEY\""));
    assert!(log.contains("\"backend\":\"self-issued\""));
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(log.contains("\"value_sha256\""));
    assert!(
        !log.contains(written.trim()),
        "raw value leaked into audit log"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("secrets/api-key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn go_rotation_name_filter_rotates_only_selected_secret() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let raw = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    let raw = raw
        .replace(
            "[secret.API_KEY]\nbackend = \"manual\"",
            "[secret.API_KEY]\nbackend = \"self-issued\"\ntarget_path = \"secrets/api-key\"",
        )
        .replace(
            "[secret.DATABASE_URL]\nbackend = \"manual\"",
            "[secret.DATABASE_URL]\nbackend = \"self-issued\"\ntarget_path = \"secrets/database-url\"",
        );
    std::fs::write(manifest_path(dir.path()), &raw).unwrap();

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--name", "API_KEY", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(dir.path().join("secrets/api-key").is_file());
    assert!(!dir.path().join("secrets/database-url").exists());

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"name\":\"API_KEY\""));
    assert!(!log.contains("\"name\":\"DATABASE_URL\""));

    let report: ready_set_sdk::CapabilityRunReport =
        serde_json::from_str(String::from_utf8(out.stdout).unwrap().trim()).unwrap();
    assert_eq!(report.actions.len(), 1);
    assert_eq!(report.actions[0].path.as_deref(), Some("secrets/api-key"));
}

#[test]
fn go_rotation_name_filter_rejects_unknown_secret() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--name", "MISSING", "--confirm"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("MISSING") && stderr.contains("not present in the manifest"),
        "expected unknown-secret error, got: {stderr}"
    );
}

#[test]
fn go_rotation_confirm_for_manual_appends_reminded_entry() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let raw = std::fs::read_to_string(manifest_path(dir.path())).unwrap();
    let raw = raw.replace(
        "[secret.API_KEY]\nbackend = \"manual\"",
        "[secret.API_KEY]\nbackend = \"manual\"\ndashboard_url = \"https://example.com/keys\"",
    );
    std::fs::write(manifest_path(dir.path()), &raw).unwrap();

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success());

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"reminded\""));
    assert!(log.contains("\"backend\":\"manual\""));
    assert!(
        !log.contains("\"value_sha256\""),
        "manual outcomes must omit value_sha256"
    );
}

#[test]
fn audit_log_is_gitignored() {
    let dir = project_with_refs();
    Command::new(plugin())
        .args(["__set", "secrets"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let gi = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
    assert!(gi.contains(".ready-set/plugins/secrets/rotations.jsonl"));
}

// ---------------------------------------------------------------------------
// v0.3: exec backend + macOS sandbox
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn project_with_one_var() -> tempfile::TempDir {
    let dir = fresh_project();
    write(
        &dir.path().join("src/main.rs"),
        "fn main() { let _ = std::env::var(\"X_TOKEN\").unwrap(); }\n",
    );
    dir
}

#[cfg(target_os = "macos")]
fn outside_sandbox_probe(name: &str) -> std::io::Result<(tempfile::TempDir, PathBuf)> {
    let allowed_tmp = std::env::temp_dir();
    let mut candidates = vec![PathBuf::from("/private/tmp")];
    if let Some(parent) = allowed_tmp.parent() {
        candidates.push(parent.to_path_buf());
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        candidates.push(home);
    }

    for root in candidates {
        let Ok(dir) = tempfile::Builder::new()
            .prefix("ready-set-encrypt-e2e-")
            .tempdir_in(&root)
        else {
            continue;
        };
        if path_is_inside(dir.path(), &allowed_tmp) {
            continue;
        }
        let probe = dir.path().join(name);
        return Ok((dir, probe));
    }

    Err(std::io::Error::other(format!(
        "could not create a panic-cleaned macOS sandbox probe outside {}",
        allowed_tmp.display()
    )))
}

#[cfg(target_os = "macos")]
fn path_is_inside(path: &Path, root: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path == root || path.starts_with(root)
}

#[cfg(target_os = "macos")]
fn sandbox_exec_unavailable(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr);
    stderr.contains("sandbox_apply: Operation not permitted")
}

#[cfg(unix)]
fn install_manifest(dir: &Path, secret_body: &str) {
    let path = manifest_path(dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body = format!(
        "[ready-set-encrypt]\nschema_version = 1\ndefault_cadence_days = 90\n\n[secret.X_TOKEN]\n{secret_body}\n"
    );
    std::fs::write(&path, body).unwrap();
}

#[cfg(unix)]
#[test]
fn self_issued_with_deploy_commands_runs_them_after_target_path_write() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"self-issued\"\nunsandboxed = true\ntarget_path = \"secrets/x-token\"\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"cp {} {}\"]]",
            dir.path().join("secrets/x-token").display(),
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let target = std::fs::read_to_string(dir.path().join("secrets/x-token")).unwrap();
    assert_eq!(target.trim().len(), 64);

    let copied = std::fs::read_to_string(&result_path).unwrap();
    assert_eq!(copied.trim(), target.trim());

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"deploy_count\":1"));
    assert!(log.contains("\"outcome\":\"rotated\""));
}

#[cfg(unix)]
#[test]
fn exec_backend_generates_and_deploys() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf deadbeef\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"printf '{{{{value}}}}' > {}\"]]",
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let copied = std::fs::read_to_string(&result_path).unwrap();
    assert_eq!(copied, "deadbeef");

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"backend\":\"exec\""));
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(log.contains("\"deploy_count\":1"));
}

#[cfg(unix)]
#[test]
fn exec_backend_failed_generate_skips_deploys() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/usr/bin/false\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"touch {}\"]]",
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!result_path.exists());

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    assert!(log.contains("\"backend\":\"exec\""));
}

#[cfg(unix)]
#[test]
fn exec_backend_failed_deploy_halts_subsequent_deploys() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf ok\"]\ndeploy_commands = [[\"/usr/bin/false\"], [\"/bin/sh\", \"-c\", \"touch {}\"]]",
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!result_path.exists(), "second deploy should not have run");

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    assert!(log.contains("deploy[0]"));
}

#[cfg(unix)]
#[test]
fn exec_backend_substitutes_value_path() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf via-path\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"cp {{{{value_path}}}} {}\"]]",
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let copied = std::fs::read_to_string(&result_path).unwrap();
    assert_eq!(copied, "via-path");
}

#[cfg(unix)]
#[test]
fn audit_never_contains_generate_command_stdout() {
    let dir = project_with_one_var();
    install_manifest(
        dir.path(),
        "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf SECRET_LEAK_TOKEN_XYZ\"]",
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(
        !log.contains("SECRET_LEAK_TOKEN_XYZ"),
        "raw value leaked into audit log"
    );
    assert!(log.contains("\"value_sha256\""));
}

#[cfg(unix)]
#[test]
fn manifest_rejects_exec_without_generate_command() {
    let dir = project_with_one_var();
    install_manifest(dir.path(), "backend = \"exec\"");

    let out = Command::new(plugin())
        .args(["__ready", "rotation"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("generate_command"),
        "expected validation error, got: {stderr}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_blocks_write_outside_project_root() {
    let dir = project_with_one_var();
    let Ok((_probe_dir, probe)) = outside_sandbox_probe("blocked-write") else {
        return;
    };

    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf ok\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"echo pwn > {}\"]]",
            probe.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    if sandbox_exec_unavailable(&out.stderr) {
        eprintln!("skipping: sandbox-exec unavailable in this environment");
        return;
    }
    assert!(
        !out.status.success(),
        "sandbox should have blocked the write"
    );
    assert!(
        !probe.exists(),
        "sandbox failed to block write outside project_root"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    assert!(log.contains("\"sandboxed\":true"));
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_allows_writes_inside_project_root() {
    let dir = project_with_one_var();
    let result_path = dir.path().join("inside-result.txt");
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf ok\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"printf '{{{{value}}}}' > {}\"]]",
            result_path.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    if sandbox_exec_unavailable(&out.stderr) {
        eprintln!("skipping: sandbox-exec unavailable in this environment");
        return;
    }
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(result_path.exists());

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"sandboxed\":true"));
    assert!(log.contains("\"platform_sandbox\":\"macos-sandbox-exec\""));
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_skipped_when_unsandboxed_set() {
    let dir = project_with_one_var();
    let Ok((_probe_dir, probe)) = outside_sandbox_probe("unsandboxed-write") else {
        return;
    };

    install_manifest(
        dir.path(),
        &format!(
            "backend = \"exec\"\nunsandboxed = true\ngenerate_command = [\"/bin/sh\", \"-c\", \"printf ok\"]\ndeploy_commands = [[\"/bin/sh\", \"-c\", \"echo bypass > {}\"]]",
            probe.display()
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "unsandboxed deploy should have succeeded; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        probe.exists(),
        "unsandboxed deploy should have written the probe"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"sandboxed\":false"));
}

// ---------------------------------------------------------------------------
// Webhook backend e2e
// ---------------------------------------------------------------------------

/// Spawn a tiny single-shot HTTP server on a random localhost port.
/// `respond_with` is the entire HTTP/1.1 response (status line, headers,
/// blank line, body). Returns the listening port; the handler thread
/// exits after serving one request. Discards the captured body — see
/// `spawn_oneshot_http_recording` when assertions on the request body
/// are needed.
#[cfg(unix)]
fn spawn_oneshot_http(respond_with: String) -> u16 {
    spawn_oneshot_http_recording(respond_with).0
}

#[cfg(unix)]
#[test]
fn webhook_generate_mode_writes_target_path_from_response() {
    let dir = project_with_one_var();
    let body = r#"{"data":{"token":"hunter2-abcdef-xyzzy"}}"#;
    let http = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    );
    let port = spawn_oneshot_http(http);
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"webhook\"\nwebhook_url = \"http://127.0.0.1:{port}/rotate\"\nwebhook_response_key = \"data.token\"\ntarget_path = \"secrets/x-token\""
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let written = std::fs::read_to_string(dir.path().join("secrets/x-token")).unwrap();
    assert_eq!(written.trim(), "hunter2-abcdef-xyzzy");

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"backend\":\"webhook\""));
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(log.contains("\"value_sha256\""));
    assert!(
        !log.contains("hunter2-abcdef-xyzzy"),
        "raw value leaked into audit log"
    );
}

#[cfg(unix)]
#[test]
fn webhook_trigger_mode_records_triggered_outcome() {
    let dir = project_with_one_var();
    let http = "HTTP/1.1 202 Accepted\r\nContent-Length: 0\r\n\r\n".to_owned();
    let port = spawn_oneshot_http(http);
    install_manifest(
        dir.path(),
        &format!("backend = \"webhook\"\nwebhook_url = \"http://127.0.0.1:{port}/trigger\""),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"backend\":\"webhook\""));
    assert!(log.contains("\"outcome\":\"triggered\""));
    assert!(
        !log.contains("\"value_sha256\""),
        "trigger mode must not record value_sha256"
    );
}

#[cfg(unix)]
#[test]
fn webhook_failure_on_4xx_records_failed() {
    let dir = project_with_one_var();
    let body = r#"{"error":"unauthorized"}"#;
    let http = format!(
        "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body,
    );
    let port = spawn_oneshot_http(http);
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"webhook\"\nwebhook_url = \"http://127.0.0.1:{port}/rotate\"\nwebhook_response_key = \"data.token\""
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "401 should propagate as failure");

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
}

#[cfg(unix)]
#[test]
fn webhook_manifest_without_url_is_rejected() {
    let dir = project_with_one_var();
    install_manifest(dir.path(), "backend = \"webhook\"");

    let out = Command::new(plugin())
        .args(["__ready", "rotation"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("webhook_url"),
        "expected validation error, got: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn webhook_does_not_follow_redirects_ssrf_defense() {
    // SSRF defense: if a configured webhook responds with a 3xx Location
    // header pointing at a different host (e.g. localhost:6379, the
    // cloud metadata service at 169.254.169.254), we MUST NOT follow.
    // The webhook URL is the contract; redirects let the upstream pick
    // an arbitrary internal target.
    let dir = project_with_one_var();
    // 302 to a port we KNOW isn't listening — if the redirect were
    // followed, we'd see a connection-refused error and could not
    // distinguish from a real failure. With redirects(0), ureq returns
    // a 302 response and we treat it as a non-2xx → outcome: failed
    // (because response_key extraction fails on an empty body).
    let http =
        "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/redirected\r\nContent-Length: 0\r\n\r\n"
            .to_owned();
    let port = spawn_oneshot_http(http);
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"webhook\"\nwebhook_url = \"http://127.0.0.1:{port}/initial\"\nwebhook_response_key = \"data.token\""
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Either the 302 surfaces as a status error or the empty body fails
    // JSON parse — both prove we did NOT follow to port 1.
    assert!(
        !out.status.success(),
        "302 should not be silently followed; stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    // The audit error string must NOT mention the redirect target at
    // port 1 — that would prove we followed.
    assert!(
        !log.contains("127.0.0.1:1") && !log.contains("/redirected"),
        "audit log mentions the redirect target — we followed when we shouldn't: {log}"
    );
}

// ---------------------------------------------------------------------------
// deploy_webhooks: HTTP-based deploy targets alongside deploy_commands
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(unix)]
fn parse_content_length(headers: &[u8]) -> usize {
    let text = std::str::from_utf8(headers).unwrap_or("");
    for line in text.split("\r\n") {
        if let Some(v) = line.strip_prefix("Content-Length:") {
            return v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.strip_prefix("content-length:") {
            return v.trim().parse().unwrap_or(0);
        }
    }
    0
}

/// Spawn a one-shot HTTP server that records the incoming request body so
/// tests can assert on `{{value}}` substitution. Returns the listening port
/// and a handle whose `.take()` yields the body bytes the server saw.
///
/// After writing the canned response, the server half-closes the write
/// side (so the client sees EOF on the response) and then drains any
/// remaining client bytes before dropping the stream. Without this
/// sequencing, ureq sometimes sees a connection reset mid-response and
/// surfaces it as "Error encountered in a header: Invalid argument".
#[cfg(unix)]
fn spawn_oneshot_http_recording(
    respond_with: String,
) -> (u16, std::sync::Arc<std::sync::Mutex<Option<Vec<u8>>>>) {
    use std::io::{Read, Write};
    use std::net::{Shutdown, TcpListener};
    use std::sync::{Arc, Mutex};

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let captured: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let captured_inner = Arc::clone(&captured);
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Read the full HTTP request before responding: ureq writes
            // headers + body in separate syscalls and races the response
            // read against its own pending writes if we reply too early.
            // Reading to the end of headers + advertised body avoids that.
            let mut buf = Vec::with_capacity(4096);
            let mut tmp = [0u8; 4096];
            let mut headers_done_at: Option<usize> = None;
            let mut content_length: usize = 0;
            loop {
                let n = match stream.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                buf.extend_from_slice(&tmp[..n]);
                if headers_done_at.is_none()
                    && let Some(end) = find_subslice(&buf, b"\r\n\r\n")
                {
                    headers_done_at = Some(end + 4);
                    content_length = parse_content_length(&buf[..end]);
                }
                if let Some(hd) = headers_done_at
                    && buf.len() >= hd + content_length
                {
                    break;
                }
            }
            *captured_inner.lock().unwrap() = Some(buf);
            drop(stream.write_all(respond_with.as_bytes()));
            drop(stream.flush());
            // Half-close write side so ureq sees EOF cleanly instead of
            // racing the connection drop.
            drop(stream.shutdown(Shutdown::Write));
            let mut sink = [0u8; 512];
            while stream.read(&mut sink).unwrap_or(0) > 0 {}
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(30));
    (port, captured)
}

#[cfg(unix)]
#[test]
fn deploy_webhook_substitutes_value_into_body_and_records_count() {
    let dir = project_with_one_var();
    let (port, captured) =
        spawn_oneshot_http_recording("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned());
    // exec backend produces a predictable value so the test can assert
    // it appears verbatim in the POST body. Built via string replace to
    // sidestep format!'s `{{`-doubling rules colliding with TOML +
    // template-placeholder braces.
    let manifest_body = r#"backend = "exec"
unsandboxed = true
generate_command = ["/bin/echo", "-n", "rotated-value-xyz"]

[[secret.X_TOKEN.deploy_webhooks]]
url = "http://127.0.0.1:__PORT__/deploy"
body = '{"secret":"{{value}}"}'"#
        .replace("__PORT__", &port.to_string());
    install_manifest(dir.path(), &manifest_body);

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let audit_path = audit_log_path(dir.path());
    let audit_dump = std::fs::read_to_string(&audit_path).unwrap_or_default();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}\naudit: {audit_dump}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let body = captured
        .lock()
        .unwrap()
        .clone()
        .expect("server should have received a request");
    let body_str = String::from_utf8_lossy(&body);
    // The freshly-generated value must appear in the body, proving
    // {{value}} substitution worked.
    assert!(
        body_str.contains(r#""secret":"rotated-value-xyz""#),
        "request body did not contain substituted value; got: {body_str}"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(
        log.contains("\"deploy_webhook_count\":1"),
        "audit must record deploy_webhook_count; got: {log}"
    );
    assert!(
        !log.contains("rotated-value-xyz"),
        "raw value leaked into audit log: {log}"
    );
}

#[cfg(unix)]
#[test]
fn deploy_webhook_failure_on_5xx_halts_and_marks_failed() {
    let dir = project_with_one_var();
    let (port, _captured) = spawn_oneshot_http_recording(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n".to_owned(),
    );
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"self-issued\"\ntarget_path = \"secrets/x-token\"\n\n[[secret.X_TOKEN.deploy_webhooks]]\nurl = \"http://127.0.0.1:{port}/deploy\""
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "5xx deploy webhook should fail the rotation"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    // We attempted one webhook; success count must be 0.
    assert!(
        log.contains("\"deploy_webhook_count\":0"),
        "expected deploy_webhook_count:0, got: {log}"
    );
    // But the target_path write happened before the failed deploy.
    assert!(
        dir.path().join("secrets/x-token").exists(),
        "target_path was written before deploy ran; should still exist"
    );
}

#[cfg(unix)]
#[test]
fn deploy_webhook_runs_after_deploy_commands_in_combined_mode() {
    let dir = project_with_one_var();
    let (port, captured) = spawn_oneshot_http_recording(
        "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n".to_owned(),
    );
    // deploy_commands runs first (writes a marker file), then the webhook
    // fires. Both must succeed for the rotation to be marked rotated.
    let marker = dir.path().join("cmd-marker.txt");
    let manifest_body = r#"backend = "self-issued"
unsandboxed = true
target_path = "secrets/x-token"
deploy_commands = [["/bin/sh", "-c", "echo cmd-ran > __MARKER__"]]

[[secret.X_TOKEN.deploy_webhooks]]
url = "http://127.0.0.1:__PORT__/deploy"
body = "{{value}}""#
        .replace("__PORT__", &port.to_string())
        .replace("__MARKER__", &marker.display().to_string());
    install_manifest(dir.path(), &manifest_body);

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let audit_dump = std::fs::read_to_string(audit_log_path(dir.path())).unwrap_or_default();
    assert!(
        out.status.success(),
        "combined deploys should succeed; stdout: {}\nstderr: {}\naudit: {audit_dump}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        marker.exists(),
        "deploy_commands marker should have been written"
    );
    let received = captured
        .lock()
        .unwrap()
        .clone()
        .expect("webhook should have been called");
    let received_str = String::from_utf8_lossy(&received);
    // Body should be the 64-char hex value (32 random bytes hex-encoded).
    let split = received_str
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or("")
        .trim_end_matches('\0');
    assert_eq!(
        split.len(),
        64,
        "expected 64-hex value in body, got `{split}`"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(log.contains("\"deploy_count\":1"));
    assert!(log.contains("\"deploy_webhook_count\":1"));
}

#[cfg(unix)]
#[test]
fn deploy_webhook_command_failure_skips_webhook_phase() {
    let dir = project_with_one_var();
    // Bind a listener but never expect it to be hit — if the webhook
    // runs despite the failed command, the test would still pass on
    // the assertion below, so we additionally verify webhook_count
    // never appears.
    let (port, captured) =
        spawn_oneshot_http_recording("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned());
    install_manifest(
        dir.path(),
        &format!(
            "backend = \"self-issued\"\ntarget_path = \"secrets/x-token\"\ndeploy_commands = [[\"/bin/false\"]]\n\n[[secret.X_TOKEN.deploy_webhooks]]\nurl = \"http://127.0.0.1:{port}/never-called\""
        ),
    );

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "failed deploy_command must halt the rotation"
    );

    // Give the (never-called) listener a beat to be sure no connection
    // arrives, then assert it stayed empty.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(
        captured.lock().unwrap().is_none(),
        "deploy webhook ran despite failed deploy_command — fail-fast violated"
    );

    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"failed\""));
    // deploy_count present (commands attempted), deploy_webhook_count must
    // be absent because the webhook phase never started.
    assert!(
        !log.contains("\"deploy_webhook_count\""),
        "webhook count should not be recorded when phase was skipped: {log}"
    );
}

#[cfg(unix)]
#[test]
fn deploy_webhook_url_substitutes_env_var() {
    // Use case: the webhook URL contains a secret token (e.g. Slack
    // webhook URLs include a hash in the path). Users put it in an env
    // var rather than committing it to the manifest.
    let dir = project_with_one_var();
    let (port, captured) =
        spawn_oneshot_http_recording("HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n".to_owned());
    let var_name = "RSS_E2E_DEPLOY_WEBHOOK_URL";
    let full_url = format!("http://127.0.0.1:{port}/secret-path");
    let manifest_body = r#"backend = "self-issued"
target_path = "secrets/x-token"

[[secret.X_TOKEN.deploy_webhooks]]
url = "{{env.RSS_E2E_DEPLOY_WEBHOOK_URL}}""#;
    install_manifest(dir.path(), manifest_body);

    let out = Command::new(plugin())
        .args(["__go", "rotation", "--", "--confirm"])
        .env("READY_SET_OUTPUT", "json")
        .env(var_name, &full_url)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let audit_dump = std::fs::read_to_string(audit_log_path(dir.path())).unwrap_or_default();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}\naudit: {audit_dump}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let body = captured
        .lock()
        .unwrap()
        .clone()
        .expect("webhook should have been called via substituted URL");
    let body_str = String::from_utf8_lossy(&body);
    assert!(
        body_str.starts_with("POST /secret-path "),
        "wrong path; request was: {body_str}"
    );

    // Verify the substituted URL is NOT itself written to the audit log
    // (avoids leaking a token-bearing URL into a committed log).
    let log = std::fs::read_to_string(audit_log_path(dir.path())).unwrap();
    assert!(log.contains("\"outcome\":\"rotated\""));
    assert!(
        !log.contains("/secret-path"),
        "substituted URL leaked into audit log: {log}"
    );
}

#[cfg(unix)]
#[test]
fn deploy_webhook_manifest_rejects_non_http_url() {
    let dir = project_with_one_var();
    install_manifest(
        dir.path(),
        "backend = \"self-issued\"\n\n[[secret.X_TOKEN.deploy_webhooks]]\nurl = \"file:///etc/passwd\"",
    );

    let out = Command::new(plugin())
        .args(["__ready", "rotation"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "non-http deploy webhook url must reject"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("deploy_webhooks"),
        "expected validation error mentioning deploy_webhooks; got: {stderr}"
    );
}
