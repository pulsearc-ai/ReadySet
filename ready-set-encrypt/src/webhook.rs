//! HTTP webhook helpers used by the `webhook` rotation backend and by the
//! optional `deploy_webhooks` array attached to any value-producing entry.
//!
//! Webhook calls have two flavors:
//!
//! - **Generate mode** — the response body is parsed as JSON and a value
//!   is extracted from a dotted key path (`webhook_response_key`). That
//!   value is treated like the stdout of an `exec` `generate_command`:
//!   it gets written to `target_path` if set and fed to
//!   `deploy_commands` / `deploy_webhooks` via the usual
//!   `{{value}}` / `{{value_path}}` substitution.
//!
//! - **Trigger mode** — no `webhook_response_key`. The POST is
//!   fire-and-forget. The audit log records
//!   `outcome: "triggered"`. Used for "ping upstream to do the
//!   rotation on its side" flows.
//!
//! Headers and body values support `{{env.NAME}}` substitution so auth
//! tokens / shared secrets don't have to live in the committed manifest.
//! Deploy-mode webhooks additionally substitute `{{value}}` so the new
//! secret can be pushed in the body or signed into a header.

use std::collections::BTreeMap;
use std::time::Duration;

use ready_set_sdk::{Error, Result};
use serde_json::Value;

use crate::manifest::{DeployWebhook, SecretEntry};

const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Outcome of a webhook call from the HTTP layer's perspective.
#[derive(Debug)]
pub struct WebhookResult {
    /// Extracted value when `webhook_response_key` was set; `None` for
    /// fire-and-forget triggers and for `deploy_webhooks` (which never
    /// extract a value because the secret is already in hand).
    pub value: Option<String>,
    /// HTTP status code of the response.
    pub status: u16,
}

/// Concrete parameters for one HTTP call. Either built from a `webhook`
/// backend's manifest entry (via [`call`]) or from a `DeployWebhook`
/// (via [`deploy`]) so both code paths share the same SSRF defenses
/// and substitution semantics.
struct CallSpec<'a> {
    url: &'a str,
    method: &'a str,
    headers: Option<&'a BTreeMap<String, String>>,
    body: Option<&'a str>,
    timeout: Duration,
    response_key: Option<&'a str>,
    /// When `Some`, `{{value}}` is substituted into body + header values
    /// before sending. The `webhook` backend itself never has a value
    /// at call time (the response *produces* one), so it passes `None`.
    value: Option<&'a str>,
}

/// Perform the HTTP call described by a `webhook`-backend `entry`.
/// Validates the manifest shape, expands `{{env.NAME}}` placeholders,
/// parses JSON when a response key is configured.
///
/// # Errors
///
/// Returns [`Error::contract`] for malformed manifests (missing URL,
/// unsubstituted env vars, missing response key), [`Error::Other`] for
/// HTTP / parsing failures.
pub fn call(entry: &SecretEntry) -> Result<WebhookResult> {
    let url = entry
        .webhook_url
        .as_deref()
        .ok_or_else(|| Error::contract("webhook backend requires webhook_url"))?;
    let spec = CallSpec {
        url,
        method: entry.webhook_method.as_deref().unwrap_or("POST"),
        headers: entry.webhook_headers.as_ref(),
        body: entry.webhook_body.as_deref(),
        timeout: Duration::from_secs(
            entry
                .webhook_timeout_seconds
                .unwrap_or(DEFAULT_TIMEOUT_SECS),
        ),
        response_key: entry.webhook_response_key.as_deref(),
        value: None,
    };
    execute(&spec)
}

/// Perform the HTTP call described by a `deploy_webhooks` entry.
///
/// Substitutes `{{value}}` (and `{{env.NAME}}`) into the body and
/// header values. Deploy webhooks never extract a value — they push
/// the freshly-rotated value upstream — so the result's `value` is
/// always `None` and only the HTTP status is meaningful for success
/// classification (2xx = success).
///
/// # Errors
///
/// Returns [`Error::contract`] for missing env vars, [`Error::Other`]
/// for HTTP / I/O failures.
pub fn deploy(webhook: &DeployWebhook, value: &str) -> Result<WebhookResult> {
    let spec = CallSpec {
        url: &webhook.url,
        method: webhook.method.as_deref().unwrap_or("POST"),
        headers: webhook.headers.as_ref(),
        body: webhook.body.as_deref(),
        timeout: Duration::from_secs(webhook.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECS)),
        response_key: None,
        value: Some(value),
    };
    execute(&spec)
}

fn execute(spec: &CallSpec<'_>) -> Result<WebhookResult> {
    // URL gets env substitution only — never `{{value}}`. Secrets in URLs
    // leak via DNS, server access logs, and the process table.
    let url = substitute_all(spec.url, None)?;
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(Error::contract(format!(
            "webhook url `{url}` (after env substitution) must be http:// or https://"
        )));
    }
    let body = spec
        .body
        .map(|b| substitute_all(b, spec.value))
        .transpose()?;
    let headers = spec
        .headers
        .map(|h| substitute_env_headers(h, spec.value))
        .transpose()?
        .unwrap_or_default();

    let config = ureq::Agent::config_builder()
        .timeout_global(Some(spec.timeout))
        // Disable redirect-following entirely. The webhook URL is the
        // contract; following a 3xx Location lets a malicious upstream
        // (or a compromised one) bounce the request to
        // http://localhost:6379, the cloud metadata service at
        // 169.254.169.254, or any other host the attacker chooses.
        // `max_redirects(0)` returns the 3xx response as-is and never follows it.
        .max_redirects(0)
        .user_agent(concat!(
            "ready-set-encrypt/",
            env!("CARGO_PKG_VERSION"),
            " (webhook backend)"
        ))
        .build();
    let agent = ureq::Agent::new_with_config(config);

    let mut builder = ureq::http::Request::builder()
        .method(spec.method)
        .uri(url.as_str());
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    // `Agent::run` takes a `Request<impl AsSendBody>`; `()` is not one, so the
    // no-body (trigger) flavor sends an empty body.
    let request = builder
        .body(body.unwrap_or_default())
        .map_err(|err| Error::other(format!("building webhook request to {url} failed: {err}")))?;

    let mut response = agent
        .run(request)
        .map_err(|err| Error::other(format!("webhook call to {url} failed: {err}")))?;
    let status = response.status().as_u16();

    let Some(key_path) = spec.response_key else {
        return Ok(WebhookResult {
            value: None,
            status,
        });
    };

    let body_text = response
        .body_mut()
        .read_to_string()
        .map_err(|err| Error::other(format!("reading webhook response body: {err}")))?;
    let json: Value = serde_json::from_str(&body_text)
        .map_err(|err| Error::other(format!("webhook response is not valid JSON: {err}")))?;
    let extracted = extract_key(&json, key_path).ok_or_else(|| {
        Error::other(format!(
            "webhook response_key `{key_path}` not found or not a string"
        ))
    })?;
    Ok(WebhookResult {
        value: Some(extracted),
        status,
    })
}

/// Look up `key_path` (dotted, e.g. `data.token`) in a JSON value. Returns
/// `Some(string)` when every segment exists and the leaf is a string;
/// `None` otherwise.
fn extract_key(value: &Value, key_path: &str) -> Option<String> {
    let mut cursor = value;
    for segment in key_path.split('.') {
        cursor = cursor.get(segment)?;
    }
    cursor.as_str().map(str::to_owned)
}

/// Substitute `{{env.NAME}}` and, when `value.is_some()`, `{{value}}`
/// inside `template`. Done in one pass so e.g. an env var holding the
/// literal `{{value}}` is never re-interpreted.
fn substitute_all(template: &str, value: Option<&str>) -> Result<String> {
    substitute_all_with_env(template, value, |name| std::env::var(name))
}

fn substitute_all_with_env(
    template: &str,
    value: Option<&str>,
    mut lookup_env: impl FnMut(&str) -> std::result::Result<String, std::env::VarError>,
) -> Result<String> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let Some(open) = rest.find("{{") else {
            out.push_str(rest);
            return Ok(out);
        };
        out.push_str(&rest[..open]);
        let after_open = &rest[open + 2..];
        let close = after_open
            .find("}}")
            .ok_or_else(|| Error::contract(format!("unterminated `{{{{` in `{template}`")))?;
        let placeholder = &after_open[..close];
        if let Some(name) = placeholder.strip_prefix("env.") {
            let resolved = lookup_env(name).map_err(|_| {
                Error::contract(format!(
                    "env var `{name}` referenced via `{{{{env.{name}}}}}` is not set"
                ))
            })?;
            out.push_str(&resolved);
        } else if placeholder == "value" {
            let v = value.ok_or_else(|| {
                Error::contract(
                    "`{{value}}` is only valid in deploy_webhooks (the webhook backend has no \
                     value at call time)",
                )
            })?;
            out.push_str(v);
        } else {
            return Err(Error::contract(format!(
                "unknown placeholder `{{{{{placeholder}}}}}` in `{template}`"
            )));
        }
        rest = &after_open[close + 2..];
    }
}

fn substitute_env_headers(
    headers: &BTreeMap<String, String>,
    value: Option<&str>,
) -> Result<Vec<(String, String)>> {
    headers
        .iter()
        .map(|(k, v)| substitute_all(v, value).map(|v| (k.clone(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_key_navigates_nested_objects() {
        let v: Value = serde_json::from_str(r#"{"data":{"token":"abc123"}}"#).unwrap();
        assert_eq!(extract_key(&v, "data.token"), Some("abc123".to_owned()));
        assert_eq!(extract_key(&v, "data.missing"), None);
        assert_eq!(extract_key(&v, "missing.token"), None);
    }

    #[test]
    fn extract_key_rejects_non_string_leaf() {
        let v: Value = serde_json::from_str(r#"{"count": 5}"#).unwrap();
        assert_eq!(extract_key(&v, "count"), None);
    }

    #[test]
    fn substitute_passes_through_literal() {
        let s = substitute_all("no placeholders here", None).unwrap();
        assert_eq!(s, "no placeholders here");
    }

    #[test]
    fn substitute_expands_env_var() {
        let s = substitute_all_with_env("Bearer {{env.RSS_TEST_WEBHOOK_ONE}}", None, |name| {
            if name == "RSS_TEST_WEBHOOK_ONE" {
                Ok("value-one".into())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        })
        .unwrap();
        assert_eq!(s, "Bearer value-one");
    }

    #[test]
    fn substitute_errors_on_unset_env_var() {
        let err = substitute_all("{{env.RSS_TEST_DEFINITELY_NOT_SET_XYZ}}", None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("RSS_TEST_DEFINITELY_NOT_SET_XYZ"), "{msg}");
    }

    #[test]
    fn substitute_errors_on_unterminated() {
        let err = substitute_all("{{env.HOME", None).unwrap_err();
        assert!(err.to_string().contains("unterminated"));
    }

    #[test]
    fn substitute_expands_value_placeholder_when_provided() {
        let s = substitute_all(r#"{"secret":"{{value}}"}"#, Some("hunter2")).unwrap();
        assert_eq!(s, r#"{"secret":"hunter2"}"#);
    }

    #[test]
    fn substitute_rejects_value_placeholder_when_unsupported() {
        let err = substitute_all("{{value}}", None).unwrap_err();
        assert!(
            err.to_string().contains("only valid in deploy_webhooks"),
            "got {err}"
        );
    }

    #[test]
    fn substitute_rejects_unknown_placeholder() {
        let err = substitute_all("{{nonsense}}", Some("v")).unwrap_err();
        assert!(err.to_string().contains("unknown placeholder"), "got {err}");
    }

    #[test]
    fn substitute_mixed_value_and_env() {
        let s = substitute_all_with_env(
            "auth={{env.RSS_TEST_WEBHOOK_MIX}}; payload={{value}}",
            Some("XYZ"),
            |name| {
                if name == "RSS_TEST_WEBHOOK_MIX" {
                    Ok("abc".into())
                } else {
                    Err(std::env::VarError::NotPresent)
                }
            },
        )
        .unwrap();
        assert_eq!(s, "auth=abc; payload=XYZ");
    }
}
