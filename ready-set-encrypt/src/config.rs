//! Project-local configuration for the secrets inventory scanner.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Path to the optional project-local secrets scanner config.
pub const CONFIG_PATH: &str = ".ready-set/plugins/secrets/config.toml";

/// Ready Set secrets configuration.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecretsConfig {
    /// Inventory scanner configuration.
    #[serde(default)]
    pub inventory: InventoryConfig,
    /// Leak scanner configuration.
    #[serde(default)]
    pub leak_scan: LeakScanConfig,
    /// Encrypted bundle configuration.
    #[serde(default)]
    pub bundles: BundleConfig,
}

/// Controls which files contribute to the env-var inventory.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InventoryConfig {
    /// Project-relative files or directories to scan for env references.
    ///
    /// When empty, the whole project tree is scanned.
    #[serde(default)]
    pub include_paths: Vec<String>,
    /// Project-relative example/template files whose keys define the canonical
    /// declared set.
    ///
    /// Defaults to `.env.example`.
    #[serde(default)]
    pub declared_files: Vec<String>,
    /// Project-relative local plaintext env files. These are advisory only and
    /// are never read for values.
    ///
    /// Defaults to `.env`.
    #[serde(default)]
    pub local_files: Vec<String>,
    /// Additional env names to suppress from the canonical inventory.
    #[serde(default)]
    pub ignore_names: Vec<String>,
    /// Treat declared example/template keys as intentional even when source
    /// code does not directly reference them.
    #[serde(default)]
    pub allow_declared_orphans: bool,
}

/// Controls repository leak scanning.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LeakScanConfig {
    /// Optional external privacy-filter integration.
    #[serde(default)]
    pub privacy_filter: PrivacyFilterConfig,
}

/// Controls an optional privacy-filter secret-span detector.
///
/// `ready-set-encrypt` does not bundle a model or hosted scanner. Projects can
/// opt into this hook by pointing `command` at an adapter, including one backed
/// by the `OpenAI` API.
#[derive(Debug, Clone, Deserialize)]
pub struct PrivacyFilterConfig {
    /// Whether `ready-set go secrets` runs the privacy-filter detector.
    #[serde(default)]
    pub enabled: bool,
    /// Project-relative or absolute privacy-filter adapter command.
    #[serde(default = "default_privacy_filter_command")]
    pub command: String,
    /// Additional adapter arguments before `ReadySet`'s protocol arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional project-relative or absolute model directory hint. Hosted
    /// adapters can ignore it.
    #[serde(default = "default_privacy_filter_model_dir")]
    pub model_dir: String,
    /// Privacy-filter mode. `report` emits spans without redacted text.
    #[serde(default = "default_privacy_filter_mode")]
    pub mode: String,
    /// Ask adapters that support fixtures to use deterministic regex matching.
    #[serde(default)]
    pub fixture_regex: bool,
}

impl Default for PrivacyFilterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: default_privacy_filter_command(),
            args: Vec::new(),
            model_dir: default_privacy_filter_model_dir(),
            mode: default_privacy_filter_mode(),
            fixture_regex: false,
        }
    }
}

/// Controls `ReadySet` encrypted secret bundles.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BundleConfig {
    /// Whether encrypted bundles are managed for this project.
    #[serde(default)]
    pub enabled: bool,
    /// Optional local key file path. Relative paths are resolved from the
    /// project root. When absent, the key is expected from `key_env`.
    #[serde(default)]
    pub key_file: Option<String>,
    /// Environment variable containing the one-time-saved bundle key.
    #[serde(default = "default_bundle_key_env")]
    pub key_env: String,
    /// Runtime environment injection configuration.
    #[serde(default)]
    pub runtime: BundleRuntimeConfig,
    /// Configured bundle files.
    #[serde(default)]
    pub files: Vec<BundleFileConfig>,
}

/// Controls which configured bundles and names are injected at runtime.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BundleRuntimeConfig {
    /// Default environment label to load when `ready-set encrypt exec` is run
    /// without `--env`.
    #[serde(default)]
    pub default_environment: Option<String>,
    /// Optional global allow-list. When empty, all decrypted names are allowed.
    #[serde(default)]
    pub include_names: Vec<String>,
    /// Global deny-list applied after allow-lists.
    #[serde(default)]
    pub exclude_names: Vec<String>,
}

/// One plaintext-to-bundle mapping.
#[derive(Debug, Clone, Deserialize)]
pub struct BundleFileConfig {
    /// Project-relative plaintext source path.
    pub source: String,
    /// Project-relative encrypted bundle path.
    pub encrypted: String,
    /// Payload format. Currently only `dotenv` is supported.
    #[serde(default = "default_bundle_payload")]
    pub payload: String,
    /// Environment label recorded in authenticated metadata.
    #[serde(default)]
    pub environment: Option<String>,
    /// Redact plaintext values from the source after encryption.
    #[serde(default)]
    pub redact_source: bool,
    /// Whether `ready-set encrypt exec` may export this bundle into a child
    /// process environment.
    #[serde(default = "default_bundle_export")]
    pub export: bool,
    /// Optional per-bundle allow-list. When empty, all names from this bundle
    /// are allowed before global filters are applied.
    #[serde(default)]
    pub include_names: Vec<String>,
    /// Per-bundle deny-list applied after allow-lists.
    #[serde(default)]
    pub exclude_names: Vec<String>,
}

impl Default for BundleFileConfig {
    fn default() -> Self {
        Self {
            source: String::new(),
            encrypted: String::new(),
            payload: default_bundle_payload(),
            environment: None,
            redact_source: false,
            export: default_bundle_export(),
            include_names: Vec::new(),
            exclude_names: Vec::new(),
        }
    }
}

fn default_bundle_payload() -> String {
    "dotenv".into()
}

const fn default_bundle_export() -> bool {
    true
}

fn default_bundle_key_env() -> String {
    "READYSET_BUNDLE_KEY".into()
}

fn default_privacy_filter_command() -> String {
    "scripts/ready-set-privacy-filter".into()
}

fn default_privacy_filter_model_dir() -> String {
    "models/privacy-filter".into()
}

fn default_privacy_filter_mode() -> String {
    "report".into()
}

impl SecretsConfig {
    /// Load config from [`CONFIG_PATH`], returning defaults when absent.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::Error`] when the config cannot be read or parsed.
    pub fn load(root: &Path) -> std::io::Result<Self> {
        let path = root.join(CONFIG_PATH);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            },
            Err(err) => return Err(err),
        };
        toml::from_str(&raw).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {err}", path.display()),
            )
        })
    }

    /// Configured source scan roots, defaulting to the project root.
    #[must_use]
    pub fn source_roots(&self, root: &Path) -> Vec<PathBuf> {
        if self.inventory.include_paths.is_empty() {
            return vec![root.to_path_buf()];
        }
        self.inventory
            .include_paths
            .iter()
            .map(|path| root.join(path))
            .collect()
    }

    /// Configured declared env files, defaulting to `.env.example`.
    #[must_use]
    pub fn declared_files(&self, root: &Path) -> Vec<PathBuf> {
        if self.inventory.declared_files.is_empty() {
            return vec![root.join(".env.example")];
        }
        self.inventory
            .declared_files
            .iter()
            .map(|path| root.join(path))
            .collect()
    }

    /// Configured local env files, defaulting to `.env`.
    #[must_use]
    pub fn local_files(&self, root: &Path) -> Vec<PathBuf> {
        if self.inventory.local_files.is_empty() {
            return vec![root.join(".env")];
        }
        self.inventory
            .local_files
            .iter()
            .map(|path| root.join(path))
            .collect()
    }

    /// Additional ignored names as a set.
    #[must_use]
    pub fn ignored_names(&self) -> BTreeSet<String> {
        self.inventory.ignore_names.iter().cloned().collect()
    }

    /// Configured local bundle key path, returning `None` when runtime key
    /// input is expected instead.
    #[must_use]
    pub fn configured_bundle_key_file(&self, root: &Path) -> Option<PathBuf> {
        self.bundles
            .key_file
            .as_deref()
            .map(|path| resolve_config_path(root, path))
    }

    /// Configured environment variable name for runtime key input.
    #[must_use]
    pub fn bundle_key_env(&self) -> &str {
        if self.bundles.key_env.is_empty() {
            "READYSET_BUNDLE_KEY"
        } else {
            &self.bundles.key_env
        }
    }
}

fn resolve_config_path(root: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}
