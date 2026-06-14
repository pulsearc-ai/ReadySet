//! Direct CLI and helpers for `ReadySet` encrypted secret bundles.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::bundle::{
    EncryptOptions, LocalKey, PayloadFormat, create_local_key, create_local_key_file, decrypt,
    dotenv_keys, encrypt, load_local_key_file, local_key_token, parse_dotenv, parse_local_key_text,
    read_bundle_file, sha256_hex, uses_current_crypto, write_bundle_file,
};
use clap::{Parser, Subcommand};
use ready_set_sdk::ExitCode;
use ready_set_sdk::fs::atomic_write;
use serde::Serialize;

use crate::config::{BundleConfig, BundleFileConfig, BundleRuntimeConfig, SecretsConfig};

type ConfiguredFiles = Option<(Option<PathBuf>, String, Vec<BundleFileConfig>)>;

/// Redacted dotenv-level diff between a bundle and its plaintext source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlaintextDiff {
    /// Keys present in the source but absent from the existing bundle.
    pub added: Vec<String>,
    /// Keys present in the existing bundle but absent from the source.
    pub removed: Vec<String>,
    /// Keys present in both whose values differ.
    pub changed: Vec<String>,
    /// Keys whose source file still contains a non-empty plaintext value.
    pub exposed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct BundleStatusReport {
    bundles_enabled: bool,
    key_file: Option<String>,
    bundles: Vec<BundleStatus>,
}

#[derive(Debug, Clone, Serialize)]
struct BundleStatus {
    source: String,
    encrypted: String,
    key_count: usize,
    keys: Vec<String>,
    non_empty_keys: Vec<String>,
    drift: BundleDrift,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum BundleDrift {
    Clean,
    SourceRedacted,
    Changed {
        added: Vec<String>,
        changed: Vec<String>,
        removed: Vec<String>,
        exposed: Vec<String>,
    },
    SourceMissing,
    Unknown,
}

#[derive(Debug)]
struct ConfiguredExecOptions {
    environment: Option<String>,
    all_environments: bool,
    bundles: Vec<PathBuf>,
    key_file: Option<PathBuf>,
    include_names: Vec<String>,
    exclude_names: Vec<String>,
    command: Vec<OsString>,
}

impl PlaintextDiff {
    /// True when the dotenv key/value content is unchanged and no plaintext
    /// values remain exposed in a redacted source file.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && self.exposed.is_empty()
    }
}

/// Direct `ready-set-encrypt` CLI.
#[derive(Debug, Parser)]
#[command(name = "ready-set-encrypt", about, long_about = None)]
pub struct DirectCli {
    /// Direct command.
    #[command(subcommand)]
    command: DirectCommand,
}

#[derive(Debug, Subcommand)]
enum DirectCommand {
    /// Work with `ReadySet` encrypted secret bundles.
    #[command(subcommand)]
    Bundle(BundleCommand),
    /// Manage the local key that decrypts `ReadySet` secret bundles.
    #[command(subcommand)]
    Key(KeyCommand),
    /// Run a command with configured `ReadySet` bundles in its environment.
    Exec {
        /// Environment label to load, matching `environment` in bundle config.
        #[arg(long = "env", alias = "environment")]
        environment: Option<String>,
        /// Ignore environment labels and load every selected bundle.
        #[arg(long = "all-envs")]
        all_environments: bool,
        /// Explicit encrypted bundle path. Defaults to configured bundles.
        #[arg(short = 'b', long = "bundle")]
        bundles: Vec<PathBuf>,
        /// Local key file. Defaults to configured key file or `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Additional runtime allow-list entry. May be repeated.
        #[arg(long = "include", value_name = "NAME")]
        include_names: Vec<String>,
        /// Additional runtime deny-list entry. May be repeated.
        #[arg(long = "exclude", value_name = "NAME")]
        exclude_names: Vec<String>,
        /// Command and arguments, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
}

/// Bundle subcommands.
#[derive(Debug, Subcommand)]
enum BundleCommand {
    /// Create a local bundle key.
    Init {
        /// Local key file. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Local key id.
        #[arg(long, default_value = "local")]
        id: String,
    },
    /// Encrypt a plaintext dotenv file.
    Encrypt {
        /// Plaintext source file.
        source: PathBuf,
        /// Encrypted bundle output path.
        #[arg(long)]
        out: PathBuf,
        /// Local key file. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Environment label.
        #[arg(long)]
        environment: Option<String>,
        /// Payload format.
        #[arg(long, default_value = "dotenv")]
        payload: String,
    },
    /// Decrypt a bundle to stdout or a file.
    Decrypt {
        /// Encrypted bundle path.
        bundle: PathBuf,
        /// Output path. Defaults to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Local key file. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
    },
    /// Run a command with decrypted dotenv variables in its environment.
    Exec {
        /// Encrypted bundle path.
        bundle: PathBuf,
        /// Local key file. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Command and arguments, after `--`.
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
    /// Print non-secret bundle metadata.
    Inspect {
        /// Encrypted bundle path.
        bundle: PathBuf,
    },
    /// Report configured bundles, captured keys, and redacted source drift.
    Status {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Local bundle key subcommands.
#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// Generate a local bundle key, print it once, and do not save it.
    Generate {
        /// Local key id.
        #[arg(long, default_value = "local")]
        id: String,
        /// Environment variable name users should provide this key through.
        #[arg(long, default_value = "READYSET_BUNDLE_KEY")]
        env: String,
    },
    /// Print an existing key file as a one-line runtime token.
    Export {
        /// Local key file to export. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Environment variable name users should provide this key through.
        #[arg(long, default_value = "READYSET_BUNDLE_KEY")]
        env: String,
    },
    /// Delete a local key file after you have saved the key somewhere safe.
    ForgetFile {
        /// Local key file to delete. Defaults to `secrets/readyset-bundle.key`.
        #[arg(long)]
        key_file: Option<PathBuf>,
        /// Required acknowledgment that the key has been saved externally.
        #[arg(long)]
        confirm_saved: bool,
    },
}

/// Run the direct CLI.
#[must_use]
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let cli = match DirectCli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            err.print().ok();
            return ExitCode::UserError;
        },
    };

    match cli.command {
        DirectCommand::Bundle(command) => run_bundle_command(command),
        DirectCommand::Key(command) => run_key_command(command),
        DirectCommand::Exec {
            environment,
            all_environments,
            bundles,
            key_file,
            include_names,
            exclude_names,
            command,
        } => match exec_configured_direct(&ConfiguredExecOptions {
            environment,
            all_environments,
            bundles,
            key_file,
            include_names,
            exclude_names,
            command,
        }) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("ready-set-encrypt exec: {err}");
                ExitCode::UserError
            },
        },
    }
}

/// Load all configured bundle files.
///
/// # Errors
///
/// Returns an error when project config cannot be loaded.
pub fn configured_files(root: &Path) -> std::io::Result<ConfiguredFiles> {
    let config = SecretsConfig::load(root)?;
    if !config.bundles.enabled {
        return Ok(None);
    }
    Ok(Some((
        config.configured_bundle_key_file(root),
        config.bundle_key_env().to_owned(),
        config.bundles.files,
    )))
}

/// Load the configured bundle key from runtime input.
///
/// Environment input is preferred over an explicit key file. When no key file
/// is configured, `ReadySet` will not create or look for one by default.
///
/// # Errors
///
/// Returns a user-readable diagnostic when no key is available or the provided
/// key is malformed.
pub fn load_configured_key(
    key_file: Option<&Path>,
    key_env: &str,
) -> Result<(LocalKey, String), String> {
    match std::env::var(key_env) {
        Ok(raw) if !raw.trim().is_empty() => {
            let key = parse_local_key_text(&raw).map_err(|err| err.to_string())?;
            return Ok((key, format!("env:{key_env}")));
        },
        Ok(_) => {
            return Err(format!("{key_env} is set but empty"));
        },
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err(format!("{key_env} is not valid UTF-8"));
        },
        Err(std::env::VarError::NotPresent) => {},
    }

    if let Some(path) = key_file {
        let key = load_local_key_file(path).map_err(|err| err.to_string())?;
        return Ok((key, path.display().to_string()));
    }

    Err(format!(
        "bundle key not available; set {key_env} to the one-time saved key from `ready-set encrypt key generate`"
    ))
}

/// Encrypt one configured bundle mapping.
///
/// # Errors
///
/// Returns an error when the source cannot be read or encryption fails.
pub fn encrypt_configured(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<(), String> {
    let source = root.join(&file.source);
    let encrypted = root.join(&file.encrypted);
    let plaintext = if file.redact_source {
        merged_redacted_plaintext(root, key, file)?
    } else {
        std::fs::read(&source).map_err(|err| format!("{}: {err}", source.display()))?
    };
    let options = options_for_file(file)?;
    let bundle =
        encrypt(&plaintext, &options, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
    write_bundle_file(&encrypted, &bundle).map_err(|err| err.to_string())?;
    Ok(())
}

/// Redact plaintext values from a configured source file.
///
/// Returns `true` when the source file changed.
///
/// # Errors
///
/// Returns a user-readable diagnostic when the file cannot be read or written.
pub fn redact_configured_source(root: &Path, file: &BundleFileConfig) -> Result<bool, String> {
    let source_path = root.join(&file.source);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|err| format!("{}: {err}", source_path.display()))?;
    let (redacted, changed) = redact_dotenv_source(&source);
    if changed {
        atomic_write(&source_path, redacted.as_bytes())
            .map_err(|err| format!("{}: {err}", source_path.display()))?;
    }
    Ok(changed)
}

/// Read configured source dotenv keys without exposing values.
///
/// # Errors
///
/// Returns a user-readable diagnostic when the source cannot be read or parsed.
pub fn configured_source_keys(root: &Path, file: &BundleFileConfig) -> Result<Vec<String>, String> {
    let source_path = root.join(&file.source);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|err| format!("{}: {err}", source_path.display()))?;
    dotenv_keys(&source).map_err(|err| err.to_string())
}

/// Compare an existing configured bundle to its plaintext source without
/// returning secret values.
///
/// # Errors
///
/// Returns a user-readable diagnostic when either side cannot be read, parsed,
/// or decrypted.
pub fn configured_plaintext_diff(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<PlaintextDiff, String> {
    let source_path = root.join(&file.source);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|err| format!("{}: {err}", source_path.display()))?;
    let source_env = parse_dotenv(&source).map_err(|err| err.to_string())?;

    let encrypted_path = root.join(&file.encrypted);
    let bundle = read_bundle_file(&encrypted_path).map_err(|err| err.to_string())?;
    let plaintext = decrypt(&bundle, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
    let decrypted = String::from_utf8(plaintext).map_err(|err| err.to_string())?;
    let bundle_env = parse_dotenv(&decrypted).map_err(|err| err.to_string())?;

    let mut diff = PlaintextDiff::default();
    for (name, value) in &source_env {
        match bundle_env.get(name) {
            None => diff.added.push(name.clone()),
            Some(previous) if previous != value => diff.changed.push(name.clone()),
            Some(_) => {},
        }
    }
    for name in bundle_env.keys() {
        if !source_env.contains_key(name) {
            diff.removed.push(name.clone());
        }
    }
    Ok(diff)
}

/// Compare an existing configured bundle to a redacted source file.
///
/// Empty values in the source are treated as placeholders for the values
/// already encrypted in the bundle. Non-empty values are treated as plaintext
/// updates and reported without returning secret values.
///
/// # Errors
///
/// Returns a user-readable diagnostic when either side cannot be read, parsed,
/// or decrypted.
pub fn configured_redacted_plaintext_diff(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<PlaintextDiff, String> {
    let source_path = root.join(&file.source);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|err| format!("{}: {err}", source_path.display()))?;
    let source_env = parse_dotenv(&source).map_err(|err| err.to_string())?;
    let bundle_env = configured_env(root, key, file)?;

    let mut diff = PlaintextDiff::default();
    for (name, value) in &source_env {
        if env_value_is_non_empty(value) {
            diff.exposed.push(name.clone());
        }
        match bundle_env.get(name) {
            None => diff.added.push(name.clone()),
            Some(previous) if env_value_is_non_empty(value) && previous != value => {
                diff.changed.push(name.clone());
            },
            Some(_) => {},
        }
    }
    for name in bundle_env.keys() {
        if !source_env.contains_key(name) {
            diff.removed.push(name.clone());
        }
    }
    Ok(diff)
}

/// Compare a configured bundle to its source using the source redaction mode.
///
/// # Errors
///
/// Returns a user-readable diagnostic when the diff cannot be computed.
pub fn configured_effective_plaintext_diff(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<PlaintextDiff, String> {
    if file.redact_source {
        configured_redacted_plaintext_diff(root, key, file)
    } else {
        configured_plaintext_diff(root, key, file)
    }
}

/// Verify that a configured bundle decrypts and matches its source when present.
///
/// # Errors
///
/// Returns a user-readable diagnostic on failure.
pub fn verify_configured(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<String, String> {
    let encrypted_path = root.join(&file.encrypted);
    let bundle = read_bundle_file(&encrypted_path).map_err(|err| err.to_string())?;
    let plaintext = decrypt(&bundle, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
    let decrypted = String::from_utf8(plaintext).map_err(|err| err.to_string())?;
    let decrypted_keys = dotenv_keys(&decrypted).map_err(|err| err.to_string())?;

    let source_path = root.join(&file.source);
    if source_path.is_file() {
        if file.redact_source {
            let diff = configured_redacted_plaintext_diff(root, key, file)?;
            if !diff.is_empty() {
                return Err(format!(
                    "{} is stale: {} has changes or plaintext values not captured in the encrypted bundle ({}); run `ready-set encrypt`",
                    file.encrypted,
                    file.source,
                    diff_label(&diff)
                ));
            }
            return Ok(format!(
                "{} decrypts ({} dotenv key{}, source redacted, sha256 {})",
                file.encrypted,
                decrypted_keys.len(),
                if decrypted_keys.len() == 1 { "" } else { "s" },
                sha256_hex(decrypted.as_bytes())
            ));
        }
        let source_bytes = std::fs::read(&source_path)
            .map_err(|err| format!("{}: {err}", source_path.display()))?;
        if source_bytes != decrypted.as_bytes() {
            return Err(format!(
                "{} is stale: plaintext {} has changes that are not encrypted in the bundle; run `ready-set encrypt`",
                file.encrypted, file.source
            ));
        }
        let source = String::from_utf8(source_bytes)
            .map_err(|err| format!("{}: {err}", source_path.display()))?;
        let source_keys = dotenv_keys(&source).map_err(|err| err.to_string())?;
        if source_keys != decrypted_keys {
            return Err(format!(
                "{} key set does not match {}",
                file.encrypted, file.source
            ));
        }
    }

    Ok(format!(
        "{} decrypts ({} dotenv key{}, sha256 {})",
        file.encrypted,
        decrypted_keys.len(),
        if decrypted_keys.len() == 1 { "" } else { "s" },
        sha256_hex(decrypted.as_bytes())
    ))
}

/// Check whether an existing encrypted bundle exactly matches its source file.
///
/// # Errors
///
/// Returns a user-readable diagnostic when the bundle cannot be checked.
pub fn configured_plaintext_matches(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<bool, String> {
    let encrypted_path = root.join(&file.encrypted);
    let bundle = read_bundle_file(&encrypted_path).map_err(|err| err.to_string())?;
    if !uses_current_crypto(&bundle) {
        return Ok(false);
    }
    if file.redact_source {
        let diff = configured_redacted_plaintext_diff(root, key, file)?;
        if !diff.is_empty() {
            return Ok(false);
        }
        let source_path = root.join(&file.source);
        let source = std::fs::read_to_string(&source_path)
            .map_err(|err| format!("{}: {err}", source_path.display()))?;
        let (_redacted, changed) = redact_dotenv_source(&source);
        return Ok(!changed);
    }
    let source_path = root.join(&file.source);
    let source =
        std::fs::read(&source_path).map_err(|err| format!("{}: {err}", source_path.display()))?;
    let plaintext = decrypt(&bundle, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
    Ok(source == plaintext)
}

fn run_bundle_command(command: BundleCommand) -> ExitCode {
    match command {
        BundleCommand::Init { key_file, id } => {
            let path = key_file.unwrap_or_else(default_key_file);
            match create_local_key_file(&path, &id) {
                Ok(key) => {
                    println!("created {} ({})", path.display(), key.id());
                    println!("{}", local_key_backup_warning(&path));
                    ExitCode::Ok
                },
                Err(err) => {
                    eprintln!("ready-set-encrypt bundle init: {err}");
                    ExitCode::UserError
                },
            }
        },
        BundleCommand::Encrypt {
            source,
            out,
            key_file,
            environment,
            payload,
        } => {
            let key_path = key_file.unwrap_or_else(default_key_file);
            match encrypt_direct(&source, &out, &key_path, environment, &payload) {
                Ok(summary) => {
                    println!("{summary}");
                    ExitCode::Ok
                },
                Err(err) => {
                    eprintln!("ready-set-encrypt bundle encrypt: {err}");
                    ExitCode::UserError
                },
            }
        },
        BundleCommand::Decrypt {
            bundle,
            out,
            key_file,
        } => {
            let key_path = key_file.unwrap_or_else(default_key_file);
            match decrypt_direct(&bundle, out.as_deref(), &key_path) {
                Ok(()) => ExitCode::Ok,
                Err(err) => {
                    eprintln!("ready-set-encrypt bundle decrypt: {err}");
                    ExitCode::UserError
                },
            }
        },
        BundleCommand::Exec {
            bundle,
            key_file,
            command,
        } => {
            let key_path = key_file.unwrap_or_else(default_key_file);
            match exec_direct(&bundle, &key_path, &command) {
                Ok(code) => code,
                Err(err) => {
                    eprintln!("ready-set-encrypt bundle exec: {err}");
                    ExitCode::UserError
                },
            }
        },
        BundleCommand::Inspect { bundle } => match inspect_direct(&bundle) {
            Ok(()) => ExitCode::Ok,
            Err(err) => {
                eprintln!("ready-set-encrypt bundle inspect: {err}");
                ExitCode::UserError
            },
        },
        BundleCommand::Status { json } => match status_direct(json) {
            Ok(code) => code,
            Err(err) => {
                eprintln!("ready-set-encrypt bundle status: {err}");
                ExitCode::UserError
            },
        },
    }
}

fn run_key_command(command: KeyCommand) -> ExitCode {
    match command {
        KeyCommand::Generate { id, env } => match create_local_key(&id) {
            Ok(key) => {
                println!("{env}={}", local_key_token(&key));
                println!(
                    "Save this key somewhere safe. ReadySet did not save it, and existing .rsb bundles cannot be decrypted without it."
                );
                ExitCode::Ok
            },
            Err(err) => {
                eprintln!("ready-set-encrypt key generate: {err}");
                ExitCode::UserError
            },
        },
        KeyCommand::Export { key_file, env } => {
            let path = key_file.unwrap_or_else(default_key_file);
            match load_local_key_file(&path) {
                Ok(key) => {
                    println!("{env}={}", local_key_token(&key));
                    println!(
                        "Save this key somewhere safe, then remove the local key file if you do not want it retrievable on this device: {}",
                        path.display()
                    );
                    ExitCode::Ok
                },
                Err(err) => {
                    eprintln!("ready-set-encrypt key export: {err}");
                    ExitCode::UserError
                },
            }
        },
        KeyCommand::ForgetFile {
            key_file,
            confirm_saved,
        } => {
            let path = key_file.unwrap_or_else(default_key_file);
            if !confirm_saved {
                eprintln!(
                    "ready-set-encrypt key forget-file: refusing to delete {}; pass --confirm-saved after saving the key externally",
                    path.display()
                );
                return ExitCode::UserError;
            }
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    println!("removed local key file {}", path.display());
                    ExitCode::Ok
                },
                Err(err) => {
                    eprintln!(
                        "ready-set-encrypt key forget-file: {}: {err}",
                        path.display()
                    );
                    ExitCode::UserError
                },
            }
        },
    }
}

fn status_direct(json: bool) -> Result<ExitCode, String> {
    let root = std::env::current_dir().map_err(|err| err.to_string())?;
    let Some((key_file, key_env, files)) =
        configured_files(&root).map_err(|err| err.to_string())?
    else {
        let report = BundleStatusReport {
            bundles_enabled: false,
            key_file: None,
            bundles: Vec::new(),
        };
        emit_status_report(&report, json)?;
        return Ok(ExitCode::Ok);
    };

    let (key, key_source) = load_configured_key(key_file.as_deref(), &key_env)?;
    let mut bundles = Vec::new();
    let mut failed = false;
    for file in &files {
        let status = configured_status(&root, &key, file);
        failed |= status.error.is_some();
        bundles.push(status);
    }
    let report = BundleStatusReport {
        bundles_enabled: true,
        key_file: Some(display_key_source(&root, key_file.as_deref(), &key_source)),
        bundles,
    };
    emit_status_report(&report, json)?;
    Ok(if failed {
        ExitCode::UserError
    } else {
        ExitCode::Ok
    })
}

fn configured_status(root: &Path, key: &LocalKey, file: &BundleFileConfig) -> BundleStatus {
    match configured_env(root, key, file) {
        Ok(env) => {
            let keys: Vec<String> = env.keys().cloned().collect();
            let non_empty_keys = env
                .iter()
                .filter(|(_name, value)| env_value_is_non_empty(value))
                .map(|(name, _value)| name.clone())
                .collect();
            BundleStatus {
                source: file.source.clone(),
                encrypted: file.encrypted.clone(),
                key_count: keys.len(),
                keys,
                non_empty_keys,
                drift: configured_drift(root, key, file),
                error: None,
            }
        },
        Err(err) => BundleStatus {
            source: file.source.clone(),
            encrypted: file.encrypted.clone(),
            key_count: 0,
            keys: Vec::new(),
            non_empty_keys: Vec::new(),
            drift: BundleDrift::Unknown,
            error: Some(err),
        },
    }
}

fn configured_env(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<BTreeMap<String, String>, String> {
    let encrypted_path = root.join(&file.encrypted);
    let bundle = read_bundle_file(&encrypted_path).map_err(|err| err.to_string())?;
    let plaintext = decrypt(&bundle, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
    let decrypted = String::from_utf8(plaintext).map_err(|err| err.to_string())?;
    parse_dotenv(&decrypted).map_err(|err| err.to_string())
}

fn configured_drift(root: &Path, key: &LocalKey, file: &BundleFileConfig) -> BundleDrift {
    if !root.join(&file.source).is_file() {
        return BundleDrift::SourceMissing;
    }
    match configured_effective_plaintext_diff(root, key, file) {
        Ok(diff) if diff.is_empty() && file.redact_source => BundleDrift::SourceRedacted,
        Ok(diff) if diff.is_empty() => BundleDrift::Clean,
        Ok(diff) => BundleDrift::Changed {
            added: diff.added,
            changed: diff.changed,
            removed: diff.removed,
            exposed: diff.exposed,
        },
        Err(_) => BundleDrift::Unknown,
    }
}

fn emit_status_report(report: &BundleStatusReport, json: bool) -> Result<(), String> {
    if json {
        let raw = serde_json::to_string_pretty(report).map_err(|err| err.to_string())?;
        println!("{raw}");
        return Ok(());
    }

    if !report.bundles_enabled {
        println!("ready-set encrypt status: encrypted bundles are disabled");
        return Ok(());
    }
    println!("ready-set encrypt status");
    if let Some(key_file) = &report.key_file {
        println!("  key-file: {key_file}");
    }
    for bundle in &report.bundles {
        println!("  bundle: {} <- {}", bundle.encrypted, bundle.source);
        if let Some(err) = &bundle.error {
            println!("    error: {err}");
            continue;
        }
        println!(
            "    keys: {} ({})",
            bundle.key_count,
            truncated_list(&bundle.keys)
        );
        println!(
            "    non-empty: {} ({})",
            bundle.non_empty_keys.len(),
            truncated_list(&bundle.non_empty_keys)
        );
        println!("    drift: {}", drift_label(&bundle.drift));
    }
    Ok(())
}

fn drift_label(drift: &BundleDrift) -> String {
    match drift {
        BundleDrift::Clean => "clean".into(),
        BundleDrift::SourceRedacted => "source redacted; bundle clean".into(),
        BundleDrift::SourceMissing => "source missing".into(),
        BundleDrift::Unknown => "unknown".into(),
        BundleDrift::Changed {
            added,
            changed,
            removed,
            exposed,
        } => {
            let mut parts = Vec::new();
            if !added.is_empty() {
                parts.push(format!("added: {}", truncated_list(added)));
            }
            if !changed.is_empty() {
                parts.push(format!("changed: {}", truncated_list(changed)));
            }
            if !removed.is_empty() {
                parts.push(format!("removed: {}", truncated_list(removed)));
            }
            if !exposed.is_empty() {
                parts.push(format!("plaintext exposed: {}", truncated_list(exposed)));
            }
            parts.join("; ")
        },
    }
}

fn truncated_list(items: &[String]) -> String {
    if items.is_empty() {
        return "-".into();
    }
    items.join(", ")
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn display_key_source(root: &Path, key_file: Option<&Path>, key_source: &str) -> String {
    if let Some(path) = key_file
        && key_source == path.display().to_string()
    {
        return display_path(root, path);
    }
    key_source.to_owned()
}

fn env_value_is_non_empty(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return !trimmed[1..trimmed.len() - 1].trim().is_empty();
    }
    true
}

fn merged_redacted_plaintext(
    root: &Path,
    key: &LocalKey,
    file: &BundleFileConfig,
) -> Result<Vec<u8>, String> {
    let source_path = root.join(&file.source);
    let source = std::fs::read_to_string(&source_path)
        .map_err(|err| format!("{}: {err}", source_path.display()))?;
    let source_env = parse_dotenv(&source).map_err(|err| err.to_string())?;
    let encrypted_path = root.join(&file.encrypted);
    let existing_env = if encrypted_path.is_file() {
        configured_env(root, key, file)?
    } else {
        BTreeMap::new()
    };

    let mut merged = BTreeMap::new();
    for (name, value) in &source_env {
        if env_value_is_non_empty(value) {
            merged.insert(name.clone(), value.clone());
        } else if let Some(previous) = existing_env.get(name) {
            merged.insert(name.clone(), previous.clone());
        } else {
            merged.insert(name.clone(), String::new());
        }
    }

    if !encrypted_path.is_file() && merged.values().all(|value| !env_value_is_non_empty(value)) {
        return Err(format!(
            "{} has no plaintext values to encrypt; add values, then run `ready-set encrypt`",
            file.source
        ));
    }

    Ok(render_dotenv_env(&merged))
}

fn render_dotenv_env(env: &BTreeMap<String, String>) -> Vec<u8> {
    let mut out = String::new();
    for (key, value) in env {
        out.push_str(key);
        out.push('=');
        if env_value_is_non_empty(value) {
            out.push('"');
            out.push_str(&escape_dotenv_double_quoted(value));
            out.push('"');
        }
        out.push('\n');
    }
    out.into_bytes()
}

fn escape_dotenv_double_quoted(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn redact_dotenv_source(source: &str) -> (String, bool) {
    let mut out = String::with_capacity(source.len());
    let mut changed = false;
    let mut seen = BTreeSet::new();

    for raw_line in source.split_inclusive('\n') {
        let (line, newline) = split_newline(raw_line);
        match redacted_dotenv_line(line) {
            Some((key, redacted)) => {
                if !seen.insert(key) {
                    changed = true;
                    continue;
                }
                changed |= redacted != line;
                out.push_str(&redacted);
                out.push_str(newline);
            },
            None => out.push_str(raw_line),
        }
    }

    (out, changed)
}

fn split_newline(line: &str) -> (&str, &str) {
    let Some(line) = line.strip_suffix('\n') else {
        return (line, "");
    };
    line.strip_suffix('\r')
        .map_or((line, "\n"), |line| (line, "\r\n"))
}

fn redacted_dotenv_line(line: &str) -> Option<(String, String)> {
    let trimmed_start = line.trim_start();
    if trimmed_start.is_empty() || trimmed_start.starts_with('#') {
        return None;
    }
    let leading = &line[..line.len() - trimmed_start.len()];
    let (export, assignment) = trimmed_start
        .strip_prefix("export ")
        .map_or(("", trimmed_start), |assignment| ("export ", assignment));
    let (raw_key, raw_value) = assignment.split_once('=')?;
    let key = raw_key.trim();
    if !is_env_name(key) {
        return None;
    }
    let redacted = format!("{leading}{export}{key}=");
    if raw_value.is_empty() && redacted == line {
        return Some((key.to_owned(), line.to_owned()));
    }
    Some((key.to_owned(), redacted))
}

fn is_env_name(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) if first == '_' || first.is_ascii_alphabetic() => {},
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn diff_label(diff: &PlaintextDiff) -> String {
    let mut parts = Vec::new();
    if !diff.added.is_empty() {
        parts.push(format!("added: {}", truncated_list(&diff.added)));
    }
    if !diff.changed.is_empty() {
        parts.push(format!("changed: {}", truncated_list(&diff.changed)));
    }
    if !diff.removed.is_empty() {
        parts.push(format!("removed: {}", truncated_list(&diff.removed)));
    }
    if !diff.exposed.is_empty() {
        parts.push(format!(
            "plaintext exposed: {}",
            truncated_list(&diff.exposed)
        ));
    }
    if parts.is_empty() {
        "no drift".into()
    } else {
        parts.join("; ")
    }
}

fn encrypt_direct(
    source: &Path,
    out: &Path,
    key_path: &Path,
    environment: Option<String>,
    payload: &str,
) -> Result<String, String> {
    let key = load_local_key_file(key_path).map_err(|err| err.to_string())?;
    let plaintext = std::fs::read(source).map_err(|err| format!("{}: {err}", source.display()))?;
    let options = EncryptOptions {
        payload_format: payload
            .parse()
            .map_err(|err: crate::bundle::BundleError| err.to_string())?,
        source_path: Some(source.to_string_lossy().replace('\\', "/")),
        environment,
        metadata: BTreeMap::default(),
    };
    let bundle = encrypt(&plaintext, &options, &[key]).map_err(|err| err.to_string())?;
    write_bundle_file(out, &bundle).map_err(|err| err.to_string())?;
    Ok(format!(
        "encrypted {} -> {} (sha256 {})",
        source.display(),
        out.display(),
        sha256_hex(&plaintext)
    ))
}

fn decrypt_direct(bundle_path: &Path, out: Option<&Path>, key_path: &Path) -> Result<(), String> {
    let key = load_local_key_file(key_path).map_err(|err| err.to_string())?;
    let bundle = read_bundle_file(bundle_path).map_err(|err| err.to_string())?;
    let plaintext = decrypt(&bundle, &[key]).map_err(|err| err.to_string())?;
    if let Some(out) = out {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("{}: {err}", parent.display()))?;
        }
        std::fs::write(out, plaintext).map_err(|err| format!("{}: {err}", out.display()))?;
    } else {
        print!("{}", String::from_utf8_lossy(&plaintext));
    }
    Ok(())
}

fn exec_direct(
    bundle_path: &Path,
    key_path: &Path,
    command: &[OsString],
) -> Result<ExitCode, String> {
    let key = load_local_key_file(key_path).map_err(|err| err.to_string())?;
    let bundle = read_bundle_file(bundle_path).map_err(|err| err.to_string())?;
    let plaintext = decrypt(&bundle, &[key]).map_err(|err| err.to_string())?;
    let dotenv = String::from_utf8(plaintext).map_err(|err| err.to_string())?;
    let env = parse_dotenv(&dotenv).map_err(|err| err.to_string())?;
    run_command_with_env(&env, command)
}

fn exec_configured_direct(options: &ConfiguredExecOptions) -> Result<ExitCode, String> {
    if options.all_environments && options.environment.is_some() {
        return Err("use either --env or --all-envs, not both".into());
    }

    let root = std::env::current_dir().map_err(|err| err.to_string())?;
    let config = SecretsConfig::load(&root).map_err(|err| err.to_string())?;
    let configured_key_file = config.configured_bundle_key_file(&root);
    let key_file = options
        .key_file
        .as_deref()
        .or(configured_key_file.as_deref());
    let (key, _source) = load_configured_key(key_file, config.bundle_key_env())?;

    let env = if options.bundles.is_empty() {
        configured_exec_env(&root, &config.bundles, &key, options)?
    } else {
        explicit_exec_env(&root, &config.bundles, &key, options)?
    };

    run_command_with_env(&env, &options.command)
}

fn configured_exec_env(
    root: &Path,
    config: &BundleConfig,
    key: &LocalKey,
    options: &ConfiguredExecOptions,
) -> Result<BTreeMap<String, String>, String> {
    if !config.enabled {
        return Err("encrypted bundles are disabled".into());
    }
    let selected_environment = options
        .environment
        .as_deref()
        .or(config.runtime.default_environment.as_deref());
    let mut env = BTreeMap::new();
    let mut selected = 0_usize;

    for file in &config.files {
        if !file.export {
            continue;
        }
        if !options.all_environments
            && selected_environment
                .is_some_and(|wanted| file.environment.as_deref() != Some(wanted))
        {
            continue;
        }
        verify_configured(root, key, file)?;
        let mut file_env = configured_env(root, key, file)?;
        apply_runtime_filters(&mut file_env, &config.runtime, file, options);
        env.extend(file_env);
        selected += 1;
    }

    if selected == 0 {
        return Err(selected_environment.map_or_else(
            || "no configured exportable bundles selected".into(),
            |environment| {
                format!("no configured exportable bundles selected for environment `{environment}`")
            },
        ));
    }

    Ok(env)
}

fn explicit_exec_env(
    root: &Path,
    config: &BundleConfig,
    key: &LocalKey,
    options: &ConfiguredExecOptions,
) -> Result<BTreeMap<String, String>, String> {
    let mut env = BTreeMap::new();
    let mut selected = 0_usize;

    for bundle_path in &options.bundles {
        let path = absolute_path(root, bundle_path);
        let bundle = read_bundle_file(&path).map_err(|err| err.to_string())?;
        if !options.all_environments
            && options
                .environment
                .as_deref()
                .is_some_and(|wanted| bundle.metadata.environment.as_deref() != Some(wanted))
        {
            continue;
        }
        let plaintext =
            decrypt(&bundle, std::slice::from_ref(key)).map_err(|err| err.to_string())?;
        let dotenv = String::from_utf8(plaintext).map_err(|err| err.to_string())?;
        let mut file_env = parse_dotenv(&dotenv).map_err(|err| err.to_string())?;
        if let Some(file) = matching_configured_file(root, config, &path) {
            if !file.export {
                continue;
            }
            apply_runtime_filters(&mut file_env, &config.runtime, file, options);
        } else {
            apply_name_filters(
                &mut file_env,
                &config.runtime.include_names,
                &config.runtime.exclude_names,
            );
            apply_name_filters(
                &mut file_env,
                &options.include_names,
                &options.exclude_names,
            );
        }
        env.extend(file_env);
        selected += 1;
    }

    if selected == 0 {
        return Err("no bundles selected for execution".into());
    }

    Ok(env)
}

fn apply_runtime_filters(
    env: &mut BTreeMap<String, String>,
    runtime: &BundleRuntimeConfig,
    file: &BundleFileConfig,
    options: &ConfiguredExecOptions,
) {
    apply_name_filters(env, &file.include_names, &file.exclude_names);
    apply_name_filters(env, &runtime.include_names, &runtime.exclude_names);
    apply_name_filters(env, &options.include_names, &options.exclude_names);
}

fn apply_name_filters(env: &mut BTreeMap<String, String>, include: &[String], exclude: &[String]) {
    if !include.is_empty() {
        let include: BTreeSet<&str> = include.iter().map(String::as_str).collect();
        env.retain(|name, _value| include.contains(name.as_str()));
    }
    if !exclude.is_empty() {
        let exclude: BTreeSet<&str> = exclude.iter().map(String::as_str).collect();
        env.retain(|name, _value| !exclude.contains(name.as_str()));
    }
}

fn matching_configured_file<'a>(
    root: &Path,
    config: &'a BundleConfig,
    bundle_path: &Path,
) -> Option<&'a BundleFileConfig> {
    config.files.iter().find(|file| {
        let configured = absolute_path(root, Path::new(&file.encrypted));
        configured == bundle_path
    })
}

fn absolute_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn run_command_with_env(
    env: &BTreeMap<String, String>,
    command: &[OsString],
) -> Result<ExitCode, String> {
    let mut env = env.clone();
    env.insert("READYSET_SECRETS_BUNDLE_ACTIVE".into(), "1".into());
    let (program, args) = command
        .split_first()
        .ok_or_else(|| "missing command after --".to_owned())?;
    let status = Command::new(program)
        .args(args)
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| format!("{}: {err}", program.to_string_lossy()))?;
    Ok(status.code().map_or(ExitCode::SystemError, |code| {
        if code == 0 {
            ExitCode::Ok
        } else {
            ExitCode::UserError
        }
    }))
}

fn inspect_direct(bundle_path: &Path) -> Result<(), String> {
    let bundle = read_bundle_file(bundle_path).map_err(|err| err.to_string())?;
    println!("format: {}", bundle.format);
    println!("version: {}", bundle.version);
    println!("payload: {:?}", bundle.payload_format);
    println!("cipher: {}", bundle.cipher);
    println!("created_at: {}", bundle.created_at);
    println!("updated_at: {}", bundle.updated_at);
    if let Some(source_path) = &bundle.metadata.source_path {
        println!("source_path: {source_path}");
    }
    if let Some(environment) = &bundle.metadata.environment {
        println!("environment: {environment}");
    }
    println!("recipients: {}", bundle.recipients.len());
    Ok(())
}

fn options_for_file(file: &BundleFileConfig) -> Result<EncryptOptions, String> {
    Ok(EncryptOptions {
        payload_format: file
            .payload
            .parse::<PayloadFormat>()
            .map_err(|err| err.to_string())?,
        source_path: Some(file.source.clone()),
        environment: file.environment.clone(),
        metadata: BTreeMap::default(),
    })
}

fn default_key_file() -> PathBuf {
    PathBuf::from("secrets/readyset-bundle.key")
}

fn local_key_backup_warning(path: &Path) -> String {
    format!(
        "Save a backup of this key somewhere safe: {}. If you lose it, existing .rsb bundles cannot be decrypted.",
        path.display()
    )
}
