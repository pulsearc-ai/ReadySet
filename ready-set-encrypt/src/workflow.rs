//! Workflow execution for `secrets` (leak scan) and `rotation` capabilities.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRunReport, CapabilityVerb, Context, Error,
    ExitCode, Output, OutputMode, Result, RunStatus,
};
use regex::Regex;
use serde::Deserialize;
use walkdir::WalkDir;

use crate::bundle_cli;
use crate::config::{PrivacyFilterConfig, SecretsConfig};
use crate::manifest::{self, Manifest};
use crate::options::RotateOptions;
use crate::rotation::{self, RotationOutcome};

/// Execute `go` for one capability in this plugin.
///
/// # Errors
///
/// Returns SDK errors when an external tool cannot be spawned or JSON output
/// cannot be written.
pub fn go_capability(
    capability: &str,
    root: &Path,
    ctx: &Context,
    args: &[OsString],
) -> Result<ExitCode> {
    match capability {
        "secrets" => go_secrets(root, ctx, args),
        "rotation" => go_rotation(root, ctx, args),
        "secret-bundles" => go_secret_bundles(root, ctx, args),
        _ => Ok(ExitCode::UserError),
    }
}

fn go_secrets(root: &Path, ctx: &Context, args: &[OsString]) -> Result<ExitCode> {
    if !args.is_empty() {
        eprintln!("ready-set-encrypt: __go secrets does not accept arguments");
        return Ok(ExitCode::UserError);
    }

    let config = SecretsConfig::load(root).map_err(Error::Io)?;
    let mut report = if which::which("gitleaks").is_ok() {
        match run_gitleaks(root) {
            Ok(report) => report,
            Err(err) => {
                eprintln!("ready-set-encrypt: gitleaks failed: {err}; falling back to regex scan");
                run_regex_scan(root)
            },
        }
    } else {
        run_regex_scan(root)
    };
    if config.leak_scan.privacy_filter.enabled {
        let privacy_report = run_privacy_filter_scan(root, &config.leak_scan.privacy_filter);
        merge_run_report(&mut report, privacy_report);
    }

    let exit_code = match report.status {
        RunStatus::Ok | RunStatus::Noop => ExitCode::Ok,
        RunStatus::Changed | RunStatus::Failed => ExitCode::UserError,
    };

    if matches!(ctx.output_mode(), OutputMode::Json) {
        let mut out = Output::for_context(ctx, std::io::stdout());
        out.json(&report)?;
    } else {
        let mut out = Output::for_context(ctx, std::io::stdout());
        render_human(&mut out, &report);
    }

    Ok(exit_code)
}

fn go_rotation(root: &Path, ctx: &Context, args: &[OsString]) -> Result<ExitCode> {
    let opts = match RotateOptions::parse_args(args) {
        Ok(opts) => opts,
        Err(err) => {
            err.print().ok();
            return Ok(ExitCode::UserError);
        },
    };

    let Some(manifest) = manifest::load(root)? else {
        let report = CapabilityRunReport {
            id: "rotation".into(),
            verb: CapabilityVerb::Go,
            status: RunStatus::Noop,
            actions: vec![CapabilityAction {
                kind: CapabilityActionKind::Skip,
                summary: "rotation manifest is missing; run `ready-set set secrets` first".into(),
                path: Some(".ready-set/plugins/secrets/manifest.toml".into()),
            }],
        };
        emit_rotation_report(ctx, &report, &opts);
        return Ok(ExitCode::UserError);
    };

    let events = rotation::read_events(root)?;
    let overdue = select_rotation_targets(&manifest, &events, &opts.names)?;

    if overdue.is_empty() {
        let summary = if opts.names.is_empty() {
            "all manifest secrets within rotation cadence".into()
        } else {
            format!(
                "selected rotation target{} not due or not rotation-tracked: {}",
                if opts.names.len() == 1 {
                    " is"
                } else {
                    "s are"
                },
                opts.names.join(", ")
            )
        };
        let report = CapabilityRunReport {
            id: "rotation".into(),
            verb: CapabilityVerb::Go,
            status: RunStatus::Noop,
            actions: vec![CapabilityAction {
                kind: CapabilityActionKind::Check,
                summary,
                path: None,
            }],
        };
        emit_rotation_report(ctx, &report, &opts);
        return Ok(ExitCode::Ok);
    }

    let (actions, stdout_lines, had_failure) =
        execute_rotations(root, &manifest, &overdue, opts.confirm);

    let status = if !opts.confirm {
        RunStatus::Noop
    } else if had_failure {
        RunStatus::Failed
    } else {
        RunStatus::Changed
    };

    let report = CapabilityRunReport {
        id: "rotation".into(),
        verb: CapabilityVerb::Go,
        status,
        actions,
    };

    if !matches!(ctx.output_mode(), OutputMode::Json) {
        for line in &stdout_lines {
            println!("{line}");
        }
    }
    emit_rotation_report(ctx, &report, &opts);

    Ok(if matches!(status, RunStatus::Failed) {
        ExitCode::UserError
    } else {
        ExitCode::Ok
    })
}

fn go_secret_bundles(root: &Path, ctx: &Context, args: &[OsString]) -> Result<ExitCode> {
    if !args.is_empty() {
        eprintln!("ready-set-encrypt: __go secret-bundles does not accept arguments");
        return Ok(ExitCode::UserError);
    }

    let mut actions = Vec::new();
    let Some((key_file, key_env, files)) = bundle_cli::configured_files(root).map_err(Error::Io)?
    else {
        let report = CapabilityRunReport {
            id: "secret-bundles".into(),
            verb: CapabilityVerb::Go,
            status: RunStatus::Noop,
            actions: vec![CapabilityAction {
                kind: CapabilityActionKind::Skip,
                summary: "encrypted bundles are disabled".into(),
                path: None,
            }],
        };
        emit_run_report(ctx, &report);
        return Ok(ExitCode::Ok);
    };

    let key = match bundle_cli::load_configured_key(key_file.as_deref(), &key_env) {
        Ok((key, _source)) => key,
        Err(err) => {
            actions.push(CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: format!("bundle key failed to load: {err}"),
                path: key_file.as_ref().map(|path| relative(root, path)),
            });
            let report = CapabilityRunReport {
                id: "secret-bundles".into(),
                verb: CapabilityVerb::Go,
                status: RunStatus::Failed,
                actions,
            };
            emit_run_report(ctx, &report);
            return Ok(ExitCode::UserError);
        },
    };

    for file in &files {
        match bundle_cli::verify_configured(root, &key, file) {
            Ok(summary) => actions.push(CapabilityAction {
                kind: CapabilityActionKind::Check,
                summary,
                path: Some(file.encrypted.clone()),
            }),
            Err(err) => actions.push(CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: err,
                path: Some(file.encrypted.clone()),
            }),
        }
    }

    let status = if actions
        .iter()
        .any(|action| action.kind == CapabilityActionKind::Error)
    {
        RunStatus::Failed
    } else if actions.is_empty() {
        RunStatus::Noop
    } else {
        RunStatus::Ok
    };
    let report = CapabilityRunReport {
        id: "secret-bundles".into(),
        verb: CapabilityVerb::Go,
        status,
        actions,
    };
    emit_run_report(ctx, &report);
    Ok(if matches!(status, RunStatus::Failed) {
        ExitCode::UserError
    } else {
        ExitCode::Ok
    })
}

fn execute_rotations(
    root: &Path,
    manifest: &Manifest,
    overdue: &[String],
    confirm: bool,
) -> (Vec<CapabilityAction>, Vec<String>, bool) {
    let mut actions: Vec<CapabilityAction> = Vec::new();
    let mut stdout_lines: Vec<String> = Vec::new();
    let mut had_failure = false;

    for name in overdue {
        let entry = manifest
            .secrets
            .get(name)
            .expect("collect_overdue guarantees presence");
        if !confirm {
            actions.push(CapabilityAction {
                kind: CapabilityActionKind::Skip,
                summary: format!(
                    "would rotate {name} via backend `{}` (pass `--confirm` to execute)",
                    entry.backend.as_str()
                ),
                path: entry.target_path.clone(),
            });
            continue;
        }
        had_failure |= rotate_one(root, name, entry, &mut actions, &mut stdout_lines);
    }

    (actions, stdout_lines, had_failure)
}

fn rotate_one(
    root: &Path,
    name: &str,
    entry: &manifest::SecretEntry,
    actions: &mut Vec<CapabilityAction>,
    stdout_lines: &mut Vec<String>,
) -> bool {
    match rotation::rotate_secret(root, name, entry) {
        Ok(RotationOutcome { event, stdout_line }) => {
            let append_failed = if let Err(err) = rotation::append_event(root, &event) {
                eprintln!("ready-set-encrypt: failed to append audit entry: {err}");
                true
            } else {
                false
            };
            let kind = match event.outcome {
                rotation::Outcome::Rotated => CapabilityActionKind::Run,
                rotation::Outcome::Reminded | rotation::Outcome::Triggered => {
                    CapabilityActionKind::Check
                },
                rotation::Outcome::Failed => CapabilityActionKind::Error,
            };
            actions.push(CapabilityAction {
                kind,
                summary: format!("{name}: {}", event.outcome_label()),
                path: event.target_path.clone(),
            });
            stdout_lines.push(stdout_line);
            append_failed || matches!(event.outcome, rotation::Outcome::Failed)
        },
        Err(err) => {
            let failure = rotation::RotationEvent {
                name: name.to_owned(),
                backend: entry.backend.as_str().to_owned(),
                ts: time::OffsetDateTime::now_utc(),
                outcome: rotation::Outcome::Failed,
                value_sha256: None,
                target_path: entry.target_path.clone(),
                error: Some(err.to_string()),
                deploy_count: None,
                deploy_webhook_count: None,
                sandboxed: None,
                platform_sandbox: None,
            };
            if let Err(append_err) = rotation::append_event(root, &failure) {
                eprintln!("ready-set-encrypt: failed to append failure audit entry: {append_err}");
            }
            actions.push(CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: format!("{name}: backend failed: {err}"),
                path: entry.target_path.clone(),
            });
            true
        },
    }
}

fn collect_overdue(manifest: &Manifest, events: &[rotation::RotationEvent]) -> Vec<String> {
    manifest
        .secrets
        .keys()
        .filter(|name| rotation::classify_cadence(manifest, events, name).needs_rotation())
        .cloned()
        .collect()
}

fn select_rotation_targets(
    manifest: &Manifest,
    events: &[rotation::RotationEvent],
    names: &[String],
) -> Result<Vec<String>> {
    if names.is_empty() {
        return Ok(collect_overdue(manifest, events));
    }

    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for name in names {
        if !manifest.secrets.contains_key(name) {
            return Err(Error::contract(format!(
                "rotation target `{name}` is not present in the manifest"
            )));
        }
        if !seen.insert(name.clone()) {
            continue;
        }
        if rotation::classify_cadence(manifest, events, name).needs_rotation() {
            out.push(name.clone());
        }
    }
    Ok(out)
}

fn emit_rotation_report(ctx: &Context, report: &CapabilityRunReport, opts: &RotateOptions) {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        if let Err(err) = out.json(report) {
            eprintln!("ready-set-encrypt: failed to emit json: {err}");
        }
        return;
    }
    let mode = if opts.confirm {
        ""
    } else {
        " (dry-run; pass --confirm)"
    };
    out.human(&format!("ready-set rotate [{:?}{mode}]", report.status));
    for action in &report.actions {
        let path = action.path.as_deref().unwrap_or("-");
        out.human(&format!(
            "  {:?}  {:<48} {}",
            action.kind, path, action.summary
        ));
    }
}

fn run_gitleaks(root: &Path) -> Result<CapabilityRunReport> {
    let output = Command::new("gitleaks")
        .args([
            "detect",
            "--no-banner",
            "--redact",
            "--report-format",
            "json",
            "--exit-code",
            "1",
        ])
        .arg("--source")
        .arg(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                Error::MissingDependency {
                    name: "gitleaks".into(),
                    hint: Some("install gitleaks or rely on the built-in regex scan".into()),
                }
            } else {
                Error::Io(err)
            }
        })?;

    let actions = parse_gitleaks_findings(&output.stdout);
    let status = if output.status.success() && actions.is_empty() {
        RunStatus::Ok
    } else {
        RunStatus::Failed
    };

    let mut all_actions = vec![CapabilityAction {
        kind: CapabilityActionKind::Run,
        summary: format!("gitleaks {}", status_summary(output.status)),
        path: None,
    }];
    all_actions.extend(actions);

    Ok(CapabilityRunReport {
        id: "secrets".into(),
        verb: CapabilityVerb::Go,
        status,
        actions: all_actions,
    })
}

fn parse_gitleaks_findings(stdout: &[u8]) -> Vec<CapabilityAction> {
    let Ok(text) = std::str::from_utf8(stdout) else {
        return Vec::new();
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        return Vec::new();
    };
    let Some(items) = value.as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .map(|item| {
            let rule = item
                .get("RuleID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let path = item.get("File").and_then(|v| v.as_str()).map(str::to_owned);
            let line = item.get("StartLine").and_then(serde_json::Value::as_u64);
            let summary = line.map_or_else(
                || format!("gitleaks rule `{rule}` matched"),
                |line| format!("gitleaks rule `{rule}` matched at line {line}"),
            );
            CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary,
                path,
            }
        })
        .collect()
}

fn run_regex_scan(root: &Path) -> CapabilityRunReport {
    let rules = builtin_rules();
    let mut findings: BTreeMap<String, Vec<(String, usize)>> = BTreeMap::new();

    for file in collect_scan_files(root) {
        for (rule_id, re) in &rules {
            for mat in re.find_iter(&file.content) {
                let line = line_number_for_byte_offset(&file.content, mat.start());
                findings
                    .entry((*rule_id).to_string())
                    .or_default()
                    .push((file.rel_path.clone(), line));
            }
        }
    }

    let mut actions: Vec<CapabilityAction> = Vec::new();
    actions.push(CapabilityAction {
        kind: CapabilityActionKind::Run,
        summary: format!(
            "built-in regex scan ({} rule{})",
            rules.len(),
            if rules.len() == 1 { "" } else { "s" }
        ),
        path: None,
    });
    for (rule_id, hits) in &findings {
        for (path, line) in hits {
            actions.push(CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: format!("rule `{rule_id}` matched at line {line}"),
                path: Some(path.clone()),
            });
        }
    }

    let status = if findings.is_empty() {
        RunStatus::Ok
    } else {
        RunStatus::Failed
    };

    CapabilityRunReport {
        id: "secrets".into(),
        verb: CapabilityVerb::Go,
        status,
        actions,
    }
}

fn run_privacy_filter_scan(root: &Path, config: &PrivacyFilterConfig) -> CapabilityRunReport {
    run_privacy_filter_scan_with(root, config, run_privacy_filter_command)
}

fn run_privacy_filter_scan_with<F>(
    root: &Path,
    config: &PrivacyFilterConfig,
    runner: F,
) -> CapabilityRunReport
where
    F: FnOnce(&Path, &PrivacyFilterConfig, &[u8]) -> std::io::Result<PrivacyFilterCommandOutput>,
{
    let files = collect_scan_files(root);
    let mut actions = vec![CapabilityAction {
        kind: CapabilityActionKind::Run,
        summary: format!(
            "privacy-filter scan ({} block{})",
            files.len(),
            if files.len() == 1 { "" } else { "s" }
        ),
        path: None,
    }];
    if files.is_empty() {
        return CapabilityRunReport {
            id: "secrets".into(),
            verb: CapabilityVerb::Go,
            status: RunStatus::Ok,
            actions,
        };
    }

    match privacy_filter_response(root, config, &files, runner) {
        Ok(response) => actions.extend(privacy_filter_actions(response, &files)),
        Err(summary) => actions.push(error_action("privacy-filter", summary)),
    }

    scan_report(actions)
}

fn privacy_filter_response(
    root: &Path,
    config: &PrivacyFilterConfig,
    files: &[ScanFile],
    runner: impl FnOnce(
        &Path,
        &PrivacyFilterConfig,
        &[u8],
    ) -> std::io::Result<PrivacyFilterCommandOutput>,
) -> std::result::Result<PrivacyFilterResponse, String> {
    let request = serde_json::json!({
        "schema": PRIVACY_FILTER_REQUEST_SCHEMA,
        "mode": config.mode,
        "model_dir": config.model_dir,
        "blocks": files
            .iter()
            .map(|file| serde_json::json!({
                "block_id": file.rel_path,
                "text": file.content,
            }))
            .collect::<Vec<_>>(),
    });
    let payload = serde_json::to_vec(&request)
        .map_err(|err| format!("failed to encode scan request: {err}"))?;
    let output = runner(root, config, &payload)
        .map_err(|err| format!("failed to run privacy-filter: {err}"))?;
    if !output.success {
        return Err(format!(
            "privacy-filter exited {}; {}",
            output.status_summary, output.stderr_summary
        ));
    }

    let response = serde_json::from_slice::<PrivacyFilterResponse>(&output.stdout)
        .map_err(|err| format!("failed to parse privacy-filter response: {err}"))?;
    if response.schema != PRIVACY_FILTER_RESPONSE_SCHEMA {
        return Err(format!(
            "privacy-filter returned unsupported schema `{}`",
            response.schema
        ));
    }

    Ok(response)
}

fn privacy_filter_actions(
    response: PrivacyFilterResponse,
    files: &[ScanFile],
) -> Vec<CapabilityAction> {
    let file_by_path: BTreeMap<_, _> = files
        .iter()
        .map(|file| (file.rel_path.as_str(), file.content.as_str()))
        .collect();
    let mut actions = Vec::new();
    for block in response.blocks {
        let Some(content) = file_by_path.get(block.block_id.as_str()) else {
            actions.push(error_action(
                "privacy-filter",
                format!("privacy-filter returned unknown block `{}`", block.block_id),
            ));
            continue;
        };
        for span in block.spans {
            if span.label != "secret" {
                continue;
            }
            let line = line_number_for_byte_offset(content, span.start_offset);
            actions.push(CapabilityAction {
                kind: CapabilityActionKind::Error,
                summary: format!("privacy-filter label `secret` matched at line {line}"),
                path: Some(block.block_id.clone()),
            });
        }
    }
    actions
}

fn run_privacy_filter_command(
    root: &Path,
    config: &PrivacyFilterConfig,
    payload: &[u8],
) -> std::io::Result<PrivacyFilterCommandOutput> {
    let command = resolve_from_root(root, &config.command);
    let mut child = Command::new(command)
        .args(&config.args)
        .arg("--mode")
        .arg(&config.mode)
        .arg("--model-dir")
        .arg(&config.model_dir)
        .args(config.fixture_regex.then_some("--fixture-regex"))
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .expect("stdin configured")
        .write_all(payload)?;
    child
        .wait_with_output()
        .map(PrivacyFilterCommandOutput::from)
}

fn scan_report(actions: Vec<CapabilityAction>) -> CapabilityRunReport {
    let status = if actions
        .iter()
        .any(|action| action.kind == CapabilityActionKind::Error)
    {
        RunStatus::Failed
    } else {
        RunStatus::Ok
    };
    CapabilityRunReport {
        id: "secrets".into(),
        verb: CapabilityVerb::Go,
        status,
        actions,
    }
}

fn error_action(path: &str, summary: String) -> CapabilityAction {
    CapabilityAction {
        kind: CapabilityActionKind::Error,
        summary,
        path: Some(path.into()),
    }
}

fn merge_run_report(report: &mut CapabilityRunReport, extra: CapabilityRunReport) {
    report.status = merge_run_status(report.status, extra.status);
    report.actions.extend(extra.actions);
}

const fn merge_run_status(left: RunStatus, right: RunStatus) -> RunStatus {
    match (left, right) {
        (RunStatus::Failed, _) | (_, RunStatus::Failed) => RunStatus::Failed,
        (RunStatus::Changed, _) | (_, RunStatus::Changed) => RunStatus::Changed,
        (RunStatus::Ok, _) | (_, RunStatus::Ok) => RunStatus::Ok,
        (RunStatus::Noop, RunStatus::Noop) => RunStatus::Noop,
    }
}

#[derive(Debug)]
struct ScanFile {
    rel_path: String,
    content: String,
}

fn collect_scan_files(root: &Path) -> Vec<ScanFile> {
    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path(), root));
    let mut files = Vec::new();
    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if is_allowlisted_file(path) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        files.push(ScanFile {
            rel_path: relative(root, path),
            content,
        });
    }
    files.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
    files
}

fn builtin_rules() -> Vec<(&'static str, Regex)> {
    let raw: &[(&str, &str)] = &[
        (
            "anthropic-api-key",
            r"sk-ant-(?:api|admin)\d{2}-[A-Za-z0-9_\-]{32,}",
        ),
        (
            "openai-project-key",
            r"sk-(?:proj|svcacct)-[A-Za-z0-9_\-]{20,}",
        ),
        ("fly-io-token", r"FlyV1 [A-Za-z0-9+/=_\-]{40,}"),
        ("cloudflare-api-token", r"cfut_[A-Za-z0-9_\-]{30,}"),
        ("slack-token", r"xox[ep](?:-[A-Za-z0-9]+){2,}"),
        ("slack-app-token", r"xapp-1-[A-Z0-9]+-[0-9]+-[A-Za-z0-9]+"),
        ("resend-api-key", r"re_[A-Za-z0-9]{8}_[A-Za-z0-9]{20,}"),
        ("aws-access-key", r"AKIA[0-9A-Z]{16}"),
        (
            "pem-block",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        ),
    ];
    raw.iter()
        .map(|(id, pat)| (*id, Regex::new(pat).expect("builtin regex")))
        .collect()
}

fn is_allowlisted_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let normalized = path.to_string_lossy().replace('\\', "/");
    matches!(name, ".env.example" | ".env.sample" | ".gitleaks.toml")
        || (normalized.contains("/deploy/secrets/")
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("rsb")))
        || path.components().any(|c| {
            matches!(
                c.as_os_str().to_str(),
                Some(".ready-set" | "node_modules" | "target")
            )
        })
}

fn is_excluded(path: &Path, root: &Path) -> bool {
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".git"
            | ".next"
            | ".turbo"
            | ".vercel"
            | ".cache"
            | ".ready-set"
            | "coverage"
            | ".venv"
            | "venv"
            | "__pycache__"
    )
}

fn relative(root: &Path, abs: &Path) -> String {
    abs.strip_prefix(root)
        .unwrap_or(abs)
        .to_string_lossy()
        .replace('\\', "/")
}

fn resolve_from_root(root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn line_number_for_byte_offset(content: &str, offset: usize) -> usize {
    content
        .char_indices()
        .take_while(|(idx, _)| *idx < offset)
        .filter(|(_, ch)| *ch == '\n')
        .count()
        + 1
}

fn status_summary(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "terminated before reporting an exit code".into(),
        |code| {
            if code == 0 {
                "no findings".into()
            } else {
                format!("findings reported (exit {code})")
            }
        },
    )
}

fn stderr_summary(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "stderr was empty".into()
    } else {
        format!("stderr: {}", trimmed.lines().next().unwrap_or(trimmed))
    }
}

const PRIVACY_FILTER_REQUEST_SCHEMA: &str = "ready_set.privacy_filter_request.v1";
const PRIVACY_FILTER_RESPONSE_SCHEMA: &str = "ready_set.privacy_filter_response.v1";

#[derive(Debug)]
struct PrivacyFilterCommandOutput {
    success: bool,
    status_summary: String,
    stdout: Vec<u8>,
    stderr_summary: String,
}

impl From<std::process::Output> for PrivacyFilterCommandOutput {
    fn from(output: std::process::Output) -> Self {
        Self {
            success: output.status.success(),
            status_summary: status_summary(output.status),
            stdout: output.stdout,
            stderr_summary: stderr_summary(&output.stderr),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PrivacyFilterResponse {
    schema: String,
    blocks: Vec<PrivacyFilterBlock>,
}

#[derive(Debug, Deserialize)]
struct PrivacyFilterBlock {
    block_id: String,
    spans: Vec<PrivacyFilterSpan>,
}

#[derive(Debug, Deserialize)]
struct PrivacyFilterSpan {
    label: String,
    start_offset: usize,
}

fn render_human(out: &mut Output, report: &CapabilityRunReport) {
    out.human(&format!(
        "ready-set-encrypt go {} ({:?})",
        report.id, report.status
    ));
    for action in &report.actions {
        let path = action.path.as_deref().unwrap_or("-");
        out.human(&format!(
            "  {:?}  {:<48} {}",
            action.kind, path, action.summary
        ));
    }
}

fn emit_run_report(ctx: &Context, report: &CapabilityRunReport) {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        if let Err(err) = out.json(report) {
            eprintln!("ready-set-encrypt: failed to emit json: {err}");
        }
        return;
    }
    render_human(&mut out, report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LeakScanConfig;

    fn privacy_filter_fixture_config() -> PrivacyFilterConfig {
        PrivacyFilterConfig {
            enabled: true,
            command: "external-openai-privacy-filter".into(),
            args: Vec::new(),
            model_dir: "models/privacy-filter".into(),
            mode: "report".into(),
            fixture_regex: true,
        }
    }

    fn run_with_fixture_privacy_filter(
        root: &Path,
        config: &PrivacyFilterConfig,
    ) -> CapabilityRunReport {
        run_privacy_filter_scan_with(root, config, |_root, _config, payload| {
            let request: serde_json::Value = serde_json::from_slice(payload).unwrap();
            assert_eq!(
                request["schema"].as_str(),
                Some(PRIVACY_FILTER_REQUEST_SCHEMA)
            );

            let blocks = request["blocks"].as_array().unwrap();
            let response_blocks = blocks
                .iter()
                .map(|block| {
                    let block_id = block["block_id"].as_str().unwrap();
                    let text = block["text"].as_str().unwrap();
                    let spans = text.find("api_key = ").map_or_else(Vec::new, |offset| {
                        vec![serde_json::json!({
                            "label": "secret",
                            "start_offset": offset,
                        })]
                    });
                    serde_json::json!({
                        "block_id": block_id,
                        "spans": spans,
                    })
                })
                .collect::<Vec<_>>();

            let response = serde_json::json!({
                "schema": PRIVACY_FILTER_RESPONSE_SCHEMA,
                "blocks": response_blocks,
            });
            Ok(PrivacyFilterCommandOutput {
                success: true,
                status_summary: "no findings".into(),
                stdout: serde_json::to_vec(&response).unwrap(),
                stderr_summary: "stderr was empty".into(),
            })
        })
    }

    #[test]
    fn regex_scan_finds_planted_anthropic_key() {
        let dir = tempfile::tempdir().unwrap();
        let leak = "sk-ant-api03-".to_owned() + &"a".repeat(64);
        std::fs::write(dir.path().join("oops.rs"), &leak).unwrap();
        let report = run_regex_scan(dir.path());
        assert_eq!(report.status, RunStatus::Failed);
        // First action is the "scan ran" header; subsequent are findings.
        let errors: Vec<_> = report
            .actions
            .iter()
            .filter(|a| a.kind == CapabilityActionKind::Error)
            .collect();
        assert!(!errors.is_empty(), "expected at least one error action");
        for action in &errors {
            assert!(
                !action.summary.contains("sk-ant-"),
                "leak bytes must not appear in output"
            );
        }
    }

    #[test]
    fn regex_scan_skips_env_example() {
        let dir = tempfile::tempdir().unwrap();
        let leak = "sk-ant-api03-".to_owned() + &"a".repeat(64);
        std::fs::write(dir.path().join(".env.example"), &leak).unwrap();
        let report = run_regex_scan(dir.path());
        assert_eq!(report.status, RunStatus::Ok);
    }

    #[test]
    fn regex_scan_skips_encrypted_secret_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deploy/secrets/root.env.rsb");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let leak = "sk-ant-api03-".to_owned() + &"a".repeat(64);
        std::fs::write(path, leak).unwrap();
        let report = run_regex_scan(dir.path());
        assert_eq!(report.status, RunStatus::Ok);
    }

    #[test]
    fn regex_scan_is_ok_when_clean() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.md"), "hello world").unwrap();
        let report = run_regex_scan(dir.path());
        assert_eq!(report.status, RunStatus::Ok);
    }

    #[test]
    fn privacy_filter_scan_finds_generic_secret() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("src/log.txt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let leak = "abcdefghijklmnop";
        std::fs::write(&path, format!("api_key = {leak}\n")).unwrap();

        let report = run_with_fixture_privacy_filter(dir.path(), &privacy_filter_fixture_config());
        assert_eq!(report.status, RunStatus::Failed);
        let errors: Vec<_> = report
            .actions
            .iter()
            .filter(|a| a.kind == CapabilityActionKind::Error)
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].path.as_deref(), Some("src/log.txt"));
        assert!(errors[0].summary.contains("line 1"));
        assert!(
            !format!("{report:?}").contains(leak),
            "leak bytes must not appear in output"
        );
    }

    #[test]
    fn privacy_filter_scan_skips_env_example() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "api_key = abcdefgh12345678\n",
        )
        .unwrap();

        let report = run_with_fixture_privacy_filter(dir.path(), &privacy_filter_fixture_config());
        assert_eq!(report.status, RunStatus::Ok);
        assert!(
            report
                .actions
                .iter()
                .all(|action| action.kind != CapabilityActionKind::Error)
        );
    }

    #[test]
    fn privacy_filter_scan_skips_encrypted_secret_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deploy/secrets/root.env.rsb");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "api_key = abcdefgh12345678\n").unwrap();

        let report = run_with_fixture_privacy_filter(dir.path(), &privacy_filter_fixture_config());
        assert_eq!(report.status, RunStatus::Ok);
        assert!(
            report
                .actions
                .iter()
                .all(|action| action.kind != CapabilityActionKind::Error)
        );
    }

    #[test]
    fn privacy_filter_is_disabled_by_default() {
        assert!(!LeakScanConfig::default().privacy_filter.enabled);
    }

    #[test]
    fn parse_gitleaks_findings_handles_empty_output() {
        assert!(parse_gitleaks_findings(b"").is_empty());
        assert!(parse_gitleaks_findings(b"   \n").is_empty());
    }

    #[test]
    fn parse_gitleaks_findings_extracts_rule_and_path() {
        let json = br#"[{"RuleID":"anthropic-api-key","File":"src/foo.rs","StartLine":12}]"#;
        let actions = parse_gitleaks_findings(json);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].path.as_deref(), Some("src/foo.rs"));
        assert!(actions[0].summary.contains("anthropic-api-key"));
        assert!(actions[0].summary.contains("line 12"));
    }
}
