//! Read-only readiness evaluation for web auth.

use std::path::Path;

use ready_set_sdk::{
    CapabilityAction, CapabilityActionKind, CapabilityRelevance, CapabilityReport, CapabilityState,
    Error, NextAction, Result,
};
use serde::Deserialize;

use crate::{CAPABILITY_ID, CAPABILITY_TITLE, PROVIDER_ID, is_auth_capability};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckLevel {
    Required,
    Advisory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCheck {
    pub id: &'static str,
    pub title: &'static str,
    pub path: Option<String>,
    pub level: CheckLevel,
    pub passed: bool,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthAudit {
    pub checks: Vec<AuthCheck>,
    pub recognized_project: bool,
}

impl AuthAudit {
    pub fn required_total(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.level == CheckLevel::Required)
            .count()
    }

    pub fn required_passed(&self) -> usize {
        self.checks
            .iter()
            .filter(|check| check.level == CheckLevel::Required && check.passed)
            .count()
    }

    pub fn required_failures(&self) -> Vec<&AuthCheck> {
        self.checks
            .iter()
            .filter(|check| check.level == CheckLevel::Required && !check.passed)
            .collect()
    }

    pub fn required_ready(&self) -> bool {
        self.required_passed() == self.required_total()
    }

    pub fn to_actions(&self) -> Vec<CapabilityAction> {
        self.checks
            .iter()
            .map(|check| CapabilityAction {
                kind: if check.passed {
                    CapabilityActionKind::Check
                } else if check.level == CheckLevel::Advisory {
                    CapabilityActionKind::Skip
                } else {
                    CapabilityActionKind::Error
                },
                summary: format!("{}: {}", check.title, check.summary),
                path: check.path.clone(),
            })
            .collect()
    }
}

const CONFIG_PATH: &str = ".ready-set/plugins/auth/config.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthReadinessConfig {
    recognize_paths: Vec<String>,
    server_sources: Vec<String>,
    route_markers: Vec<String>,
    session_markers: Vec<String>,
    identity_sources: Vec<String>,
    identity_markers: Vec<String>,
    env_examples: Vec<String>,
    env_vars: Vec<String>,
    client_sources: Vec<String>,
    login_markers: Vec<String>,
    account_policy_markers: Vec<String>,
    local_source_paths: Vec<String>,
}

impl Default for AuthReadinessConfig {
    fn default() -> Self {
        Self {
            recognize_paths: vec![
                "crates/ready-set/ready-set-auth".into(),
                "backend/Cargo.toml".into(),
                "app/src/client/pages/Login.tsx".into(),
            ],
            server_sources: vec!["backend/src/main.rs".into()],
            route_markers: vec!["oauth_start".into(), "oauth_callback".into()],
            session_markers: vec![
                "SESSION_COOKIE".into(),
                "session_cookie".into(),
                "sign_claims".into(),
                "app_session".into(),
            ],
            identity_sources: vec!["backend/src".into()],
            identity_markers: vec!["oauth_identities".into()],
            env_examples: vec![".env.example".into(), "backend/.env.example".into()],
            env_vars: vec![
                "OAUTH_GOOGLE_CLIENT_ID".into(),
                "OAUTH_GOOGLE_CLIENT_SECRET".into(),
                "OAUTH_GITHUB_CLIENT_ID".into(),
                "OAUTH_GITHUB_CLIENT_SECRET".into(),
            ],
            client_sources: vec!["app/src/client/pages/Login.tsx".into()],
            login_markers: vec![
                "Continue with Google".into(),
                "Continue with GitHub".into(),
                "window.location".into(),
            ],
            account_policy_markers: vec!["Account creation is invite-only".into()],
            local_source_paths: vec!["crates/ready-set/ready-set-auth/src/lib.rs".into()],
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct RawAuthReadinessConfig {
    recognize_paths: Option<Vec<String>>,
    server_sources: Option<Vec<String>>,
    route_markers: Option<Vec<String>>,
    session_markers: Option<Vec<String>>,
    identity_sources: Option<Vec<String>>,
    identity_markers: Option<Vec<String>>,
    env_examples: Option<Vec<String>>,
    env_vars: Option<Vec<String>>,
    client_sources: Option<Vec<String>>,
    login_markers: Option<Vec<String>>,
    account_policy_markers: Option<Vec<String>>,
    local_source_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedAuthConfig {
    config: AuthReadinessConfig,
    config_present: bool,
    error: Option<String>,
}

impl LoadedAuthConfig {
    fn load(root: &Path) -> Self {
        let path = root.join(CONFIG_PATH);
        let Some(raw) = read(&path) else {
            return Self {
                config: AuthReadinessConfig::default(),
                config_present: false,
                error: None,
            };
        };

        match toml::from_str::<RawAuthReadinessConfig>(&raw) {
            Ok(raw_config) => Self {
                config: raw_config.apply_defaults(),
                config_present: true,
                error: None,
            },
            Err(err) => Self {
                config: AuthReadinessConfig::default(),
                config_present: true,
                error: Some(format!("failed to parse {CONFIG_PATH}: {err}")),
            },
        }
    }
}

impl RawAuthReadinessConfig {
    fn apply_defaults(self) -> AuthReadinessConfig {
        let defaults = AuthReadinessConfig::default();
        AuthReadinessConfig {
            recognize_paths: self.recognize_paths.unwrap_or(defaults.recognize_paths),
            server_sources: self.server_sources.unwrap_or(defaults.server_sources),
            route_markers: self.route_markers.unwrap_or(defaults.route_markers),
            session_markers: self.session_markers.unwrap_or(defaults.session_markers),
            identity_sources: self.identity_sources.unwrap_or(defaults.identity_sources),
            identity_markers: self.identity_markers.unwrap_or(defaults.identity_markers),
            env_examples: self.env_examples.unwrap_or(defaults.env_examples),
            env_vars: self.env_vars.unwrap_or(defaults.env_vars),
            client_sources: self.client_sources.unwrap_or(defaults.client_sources),
            login_markers: self.login_markers.unwrap_or(defaults.login_markers),
            account_policy_markers: self
                .account_policy_markers
                .unwrap_or(defaults.account_policy_markers),
            local_source_paths: self
                .local_source_paths
                .unwrap_or(defaults.local_source_paths),
        }
    }
}

/// Evaluate the auth capability.
///
/// # Errors
///
/// Returns a contract error when an unknown capability id is requested.
pub fn report(capability: &str, root: &Path) -> Result<CapabilityReport> {
    if !is_auth_capability(capability) {
        return Err(Error::contract(format!(
            "unknown capability `{capability}` for provider `{PROVIDER_ID}`"
        )));
    }

    let audit = audit_project(root);
    let failures = audit.required_failures();
    let state = if !audit.recognized_project {
        CapabilityState::NotNeeded
    } else if failures.is_empty() {
        CapabilityState::Ready
    } else {
        CapabilityState::Incomplete
    };

    Ok(CapabilityReport {
        id: CAPABILITY_ID.into(),
        title: CAPABILITY_TITLE.into(),
        provider: PROVIDER_ID.into(),
        state,
        relevance: CapabilityRelevance::Required,
        summary: summary_for(&audit, state, &failures),
        next_action: next_action_for(state),
    })
}

pub fn audit_project(root: &Path) -> AuthAudit {
    let loaded = LoadedAuthConfig::load(root);
    let config = &loaded.config;
    let recognized_project =
        loaded.config_present || path_exists_any(root, &config.recognize_paths);

    let mut checks = Vec::new();
    if let Some(error) = loaded.error {
        checks.push(required_dynamic(
            "auth-plugin-config",
            "auth plugin config",
            Some(CONFIG_PATH.into()),
            false,
            "auth plugin config is valid".into(),
            error,
        ));
    }

    checks.push(required(
        "oauth-routes",
        "OAuth start/callback routes",
        first_path(&config.server_sources),
        paths_contain_all_markers(root, &config.server_sources, &config.route_markers),
        "OAuth start and callback route markers found",
        "OAuth start/callback route markers are missing",
    ));

    checks.push(required(
        "session-bridge",
        "session or account bridge",
        first_path(&config.server_sources),
        paths_contain_all_markers(root, &config.server_sources, &config.session_markers),
        "session/account bridge markers found",
        "session/account bridge markers are missing",
    ));

    checks.push(required(
        "provider-identity-storage",
        "provider identity storage",
        first_path(&config.identity_sources),
        paths_contain_all_markers(root, &config.identity_sources, &config.identity_markers),
        "provider identity storage markers found",
        "provider identity storage markers are missing",
    ));

    checks.push(required(
        "oauth-provider-config",
        "OAuth provider configuration",
        first_path(&config.env_examples),
        env_examples_have_vars(root, &config.env_examples, &config.env_vars),
        "OAuth provider env vars are documented",
        "OAuth provider env vars are missing from examples",
    ));

    checks.push(required(
        "login-entry-points",
        "login entry points",
        first_path(&config.client_sources),
        paths_contain_all_markers(root, &config.client_sources, &config.login_markers),
        "login entry point markers found",
        "login entry point markers are missing",
    ));

    checks.push(advisory(
        "local-ready-set-auth-source",
        "local ready-set-auth source",
        first_path(&config.local_source_paths),
        path_exists_any(root, &config.local_source_paths),
        "local ready-set-auth provider source is present",
        "install ready-set-auth as local tooling; do not add it to deployed app dependencies",
    ));

    checks.push(advisory(
        "invite-only-signup",
        "account creation policy",
        first_path(&config.client_sources),
        paths_contain_all_markers(root, &config.client_sources, &config.account_policy_markers),
        "password signup remains invite-only",
        "confirm the OAuth auto-provisioning policy for this product",
    ));

    AuthAudit {
        checks,
        recognized_project,
    }
}

fn summary_for(audit: &AuthAudit, state: CapabilityState, failures: &[&AuthCheck]) -> String {
    if state == CapabilityState::NotNeeded {
        return "no configured auth surface detected".into();
    }
    if failures.is_empty() {
        return format!(
            "auth required checks are ready ({}/{})",
            audit.required_passed(),
            audit.required_total()
        );
    }

    let first = failures
        .first()
        .map_or("unknown auth gap", |check| check.summary.as_str());
    format!(
        "{}/{} auth checks passed; first gap: {first}",
        audit.required_passed(),
        audit.required_total()
    )
}

fn next_action_for(state: CapabilityState) -> Option<NextAction> {
    if matches!(
        state,
        CapabilityState::Missing | CapabilityState::Incomplete
    ) {
        Some(NextAction {
            command: "ready-set set auth".into(),
            description: "Write the local auth implementation plan".into(),
        })
    } else {
        None
    }
}

fn required(
    id: &'static str,
    title: &'static str,
    path: Option<String>,
    passed: bool,
    pass: &'static str,
    fail: &'static str,
) -> AuthCheck {
    required_dynamic(id, title, path, passed, pass.into(), fail.into())
}

fn required_dynamic(
    id: &'static str,
    title: &'static str,
    path: Option<String>,
    passed: bool,
    pass: String,
    fail: String,
) -> AuthCheck {
    AuthCheck {
        id,
        title,
        path,
        level: CheckLevel::Required,
        passed,
        summary: if passed { pass } else { fail },
    }
}

fn advisory(
    id: &'static str,
    title: &'static str,
    path: Option<String>,
    passed: bool,
    pass: &'static str,
    fail: &'static str,
) -> AuthCheck {
    AuthCheck {
        id,
        title,
        path,
        level: CheckLevel::Advisory,
        passed,
        summary: if passed { pass.into() } else { fail.into() },
    }
}

fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn first_path(paths: &[String]) -> Option<String> {
    paths.first().cloned()
}

fn path_exists_any(root: &Path, paths: &[String]) -> bool {
    paths.iter().any(|path| root.join(path).exists())
}

fn paths_contain_all_markers(root: &Path, paths: &[String], markers: &[String]) -> bool {
    !markers.is_empty()
        && markers.iter().all(|marker| {
            paths
                .iter()
                .any(|path| path_contains(&root.join(path), marker))
        })
}

fn env_examples_have_vars(root: &Path, paths: &[String], vars: &[String]) -> bool {
    if vars.is_empty() {
        return false;
    }

    let mut joined = String::new();
    for path in paths {
        if let Some(raw) = read(root.join(path)) {
            joined.push_str(&raw);
            joined.push('\n');
        }
    }
    vars.iter().all(|name| joined.contains(name))
}

fn path_contains(path: &Path, needle: &str) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if metadata.is_file() {
        return read(path).is_some_and(|raw| raw.contains(needle));
    }
    if !metadata.is_dir() {
        return false;
    }
    dir_contains(path, needle)
}

fn dir_contains(path: &Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            if dir_contains(&path, needle) {
                return true;
            }
        } else if metadata.is_file() && read(&path).is_some_and(|raw| raw.contains(needle)) {
            return true;
        }
    }
    false
}
