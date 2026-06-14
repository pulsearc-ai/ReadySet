//! `ready-set-plugin` scaffold generator.
//!
//! This binary is itself a normal `ready-set-*` plugin. It creates standalone
//! plugin crates that depend on `ready-set-sdk`; it does not require authors to
//! edit the SDK or the dispatcher.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail};
use clap::{CommandFactory as _, Parser, Subcommand, ValueEnum};
use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::prelude::*;
use serde::{Deserialize, Serialize};

const BLUEPRINT_FILE: &str = "ready-set-plugin.yaml";
const DEFAULT_SDK_VERSION: &str = "0.1";
const RESERVED_PLUGIN_NAMES: &[&str] = &[
    "ready", "set", "go", "help", "list", "version", "undo", "plugin",
];
const RUST_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

#[derive(Debug, Parser)]
#[command(
    name = "ready-set-plugin",
    about = "Create ready-set plugin crates from YAML blueprints"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Create a plugin blueprint and crate from flags.
    New(NewArgs),
    /// Generate a plugin crate from a YAML blueprint.
    Generate(GenerateArgs),
    /// Validate a plugin YAML blueprint without generating files.
    Validate(ValidateArgs),
    /// Print a starter YAML blueprint to stdout.
    Init(InitArgs),
}

#[derive(Debug, Parser)]
struct NewArgs {
    /// Plugin name, with or without the ready-set- prefix.
    name: String,
    /// Output directory. Defaults to `./ready-set-NAME`.
    #[arg(long)]
    path: Option<PathBuf>,
    /// Generate a command-only plugin or a lifecycle provider.
    #[arg(long, value_enum, default_value_t = PluginKind::Provider)]
    kind: PluginKind,
    /// One-line metadata description.
    #[arg(long)]
    description: Option<String>,
    /// Capability id for provider plugins.
    #[arg(long)]
    capability: Option<String>,
    /// Capability title for provider plugins.
    #[arg(long)]
    title: Option<String>,
    /// Comma-separated lifecycle verbs for provider plugins.
    #[arg(long, default_value = "ready,go")]
    verbs: String,
    /// Optional project requirement id, repeatable.
    #[arg(long = "project-requirement")]
    project_requirements: Vec<String>,
    /// Optional user-facing alias for this plugin.
    #[arg(long)]
    alias: Option<String>,
    /// ready-set-sdk dependency version for generated Cargo.toml.
    #[arg(long, default_value = DEFAULT_SDK_VERSION)]
    sdk_version: String,
    /// Local ready-set-sdk path for generated Cargo.toml.
    #[arg(long)]
    sdk_path: Option<String>,
    /// Also generate `dist/ready-set-NAME.toml`.
    #[arg(long)]
    sidecar: bool,
    /// Run cargo fmt --check and cargo test in the generated crate.
    #[arg(long)]
    verify: bool,
}

#[derive(Debug, Parser)]
struct GenerateArgs {
    /// Path to ready-set-plugin.yaml.
    blueprint: PathBuf,
    /// Output directory. Defaults to `./ready-set-NAME`.
    #[arg(long)]
    path: Option<PathBuf>,
    /// Overwrite generated files if they already exist.
    #[arg(long)]
    force: bool,
    /// Run cargo fmt --check and cargo test in the generated crate.
    #[arg(long)]
    verify: bool,
}

#[derive(Debug, Parser)]
struct ValidateArgs {
    /// Path to ready-set-plugin.yaml.
    blueprint: PathBuf,
}

#[derive(Debug, Parser)]
struct InitArgs {
    /// Plugin name for the starter blueprint.
    name: String,
    /// Generate a command-only plugin or a lifecycle provider.
    #[arg(long, value_enum, default_value_t = PluginKind::Provider)]
    kind: PluginKind,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum PluginKind {
    Command,
    Provider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Blueprint {
    schema_version: u32,
    plugin: PluginSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    capabilities: Vec<CapabilitySpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    aliases: Vec<AliasSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<ConfigSpec>,
    #[serde(default, skip_serializing_if = "DependencySpec::is_empty")]
    dependencies: DependencySpec,
    #[serde(default, skip_serializing_if = "GenerationSpec::is_empty")]
    generation: GenerationSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginSpec {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    crate_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binary_name: Option<String>,
    description: String,
    version: String,
    stability: StabilitySpec,
    min_dispatcher_version: String,
    #[serde(default = "default_platforms")]
    platforms: Vec<PlatformSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    project_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilitySpec {
    id: String,
    title: String,
    #[serde(default = "default_relevance")]
    default_relevance: RelevanceSpec,
    verbs: Vec<VerbSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ready: Option<ReadySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    set: Option<RunSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    go: Option<RunSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_ready: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    summary_missing: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunSpec {
    #[serde(default)]
    accepts_args: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    human_success: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasSpec {
    name: String,
    description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    match_first_arg: Option<String>,
    target: AliasTargetSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capability: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    fields: Vec<ConfigField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigField {
    name: String,
    #[serde(rename = "type")]
    field_type: ConfigFieldType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<serde_yaml_ng::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencySpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    external_tools: Vec<String>,
    #[serde(default)]
    network: bool,
    #[serde(default)]
    writes_files: bool,
}

impl DependencySpec {
    const fn is_empty(&self) -> bool {
        self.external_tools.is_empty() && !self.network && !self.writes_files
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    license: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include_readme: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include_contract_tests: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include_github_ci: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    include_sidecar_manifest: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sdk_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sdk_path: Option<String>,
}

impl GenerationSpec {
    const fn is_empty(&self) -> bool {
        self.license.is_none()
            && self.include_readme.is_none()
            && self.include_contract_tests.is_none()
            && self.include_github_ci.is_none()
            && self.include_sidecar_manifest.is_none()
            && self.sdk_version.is_none()
            && self.sdk_path.is_none()
    }

    fn include_readme(&self) -> bool {
        self.include_readme.unwrap_or(true)
    }

    fn include_contract_tests(&self) -> bool {
        self.include_contract_tests.unwrap_or(true)
    }

    fn include_github_ci(&self) -> bool {
        self.include_github_ci.unwrap_or(true)
    }

    fn include_sidecar_manifest(&self) -> bool {
        self.include_sidecar_manifest.unwrap_or(false)
    }

    fn sdk_version(&self) -> &str {
        self.sdk_version.as_deref().unwrap_or(DEFAULT_SDK_VERSION)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum StabilitySpec {
    Stable,
    Experimental,
    Deprecated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum PlatformSpec {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum RelevanceSpec {
    Required,
    Optional,
    NotNeeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum VerbSpec {
    Ready,
    Set,
    Go,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum AliasTargetSpec {
    Set,
    Go,
    Plugin,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConfigFieldType {
    Bool,
    String,
    Integer,
    ArrayString,
}

fn main() -> std::process::ExitCode {
    let description = describe();
    if let Some(code) = description.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }

    match run() {
        Ok(()) => ExitCode::Ok.into(),
        Err(err) => {
            eprintln!("ready-set-plugin: {err:#}");
            ExitCode::UserError.into()
        },
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::New(args)) => new_project(args),
        Some(Commands::Generate(args)) => generate_from_file(args),
        Some(Commands::Validate(args)) => validate_from_file(&args),
        Some(Commands::Init(args)) => print_starter_blueprint(&args),
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        },
    }
}

fn describe() -> Describe {
    Describe {
        description: "Generate ready-set plugin crates from YAML blueprints".into(),
        version: env!("CARGO_PKG_VERSION").parse().unwrap_or_else(|_| {
            "0.0.0"
                .parse()
                .expect("literal semver fallback should parse")
        }),
        stability: Stability::Experimental,
        min_dispatcher_version: "0.1.0".parse().expect("literal semver should parse"),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        project_requirements: Vec::new(),
        capabilities: Vec::new(),
        command_aliases: Vec::new(),
    }
}

fn new_project(args: NewArgs) -> Result<()> {
    let mut blueprint = starter_blueprint(&args.name, args.kind)?;
    blueprint.plugin.description = args.description.unwrap_or(blueprint.plugin.description);
    blueprint.plugin.project_requirements = args.project_requirements;
    blueprint.generation.sdk_version = Some(args.sdk_version);
    blueprint.generation.sdk_path = args.sdk_path;
    blueprint.generation.include_sidecar_manifest = Some(args.sidecar);

    if matches!(args.kind, PluginKind::Provider) {
        let capability_id = args
            .capability
            .unwrap_or_else(|| blueprint.plugin.name.clone());
        validate_kebab("capability id", &capability_id)?;
        let verbs = parse_verbs(&args.verbs)?;
        let title = args.title.unwrap_or_else(|| title_case(&capability_id));
        blueprint.capabilities = vec![CapabilitySpec {
            id: capability_id.clone(),
            title,
            default_relevance: RelevanceSpec::Required,
            verbs,
            ready: Some(ReadySpec {
                summary_ready: Some(format!("{} is configured", title_case(&capability_id))),
                summary_missing: Some(format!("{} is not configured", title_case(&capability_id))),
            }),
            set: Some(RunSpec {
                accepts_args: false,
                human_success: Some(format!("{} reconciled", title_case(&capability_id))),
            }),
            go: Some(RunSpec {
                accepts_args: true,
                human_success: Some(format!("{} completed", title_case(&capability_id))),
            }),
        }];
    }

    if let Some(alias) = args.alias {
        validate_kebab("alias", &alias)?;
        let capability = blueprint.capabilities.first().map(|c| c.id.clone());
        blueprint.aliases = vec![AliasSpec {
            name: alias.clone(),
            description: format!("Run {alias}"),
            match_first_arg: None,
            target: if capability.is_some() {
                AliasTargetSpec::Go
            } else {
                AliasTargetSpec::Plugin
            },
            capability,
            args: Vec::new(),
        }];
    }

    let out_dir = args
        .path
        .unwrap_or_else(|| PathBuf::from(blueprint.crate_name()));
    generate_project(&blueprint, &out_dir, false)?;
    format_project(&out_dir)?;
    if args.verify {
        verify_project(&out_dir)?;
    }
    println!("created {}", out_dir.display());
    println!("next: cd {} && cargo test", out_dir.display());
    Ok(())
}

fn generate_from_file(args: GenerateArgs) -> Result<()> {
    let blueprint = load_blueprint(&args.blueprint)?;
    let out_dir = args
        .path
        .unwrap_or_else(|| PathBuf::from(blueprint.crate_name()));
    generate_project(&blueprint, &out_dir, args.force)?;
    format_project(&out_dir)?;
    if args.verify {
        verify_project(&out_dir)?;
    }
    println!("generated {}", out_dir.display());
    Ok(())
}

fn validate_from_file(args: &ValidateArgs) -> Result<()> {
    load_blueprint(&args.blueprint)?;
    println!("valid {}", args.blueprint.display());
    Ok(())
}

fn load_blueprint(path: &Path) -> Result<Blueprint> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let blueprint: Blueprint = serde_yaml_ng::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    blueprint.validate()?;
    Ok(blueprint)
}

fn print_starter_blueprint(args: &InitArgs) -> Result<()> {
    let blueprint = starter_blueprint(&args.name, args.kind)?;
    let yaml = serde_yaml_ng::to_string(&blueprint)?;
    print!("{yaml}");
    Ok(())
}

fn starter_blueprint(name: &str, kind: PluginKind) -> Result<Blueprint> {
    let plugin_id = normalize_plugin_name(name)?;
    let description = match kind {
        PluginKind::Command => format!("{} command plugin", title_case(&plugin_id)),
        PluginKind::Provider => format!("{} capability provider", title_case(&plugin_id)),
    };
    let capability = CapabilitySpec {
        id: plugin_id.clone(),
        title: title_case(&plugin_id),
        default_relevance: RelevanceSpec::Required,
        verbs: vec![VerbSpec::Ready, VerbSpec::Go],
        ready: Some(ReadySpec {
            summary_ready: Some(format!("{} is configured", title_case(&plugin_id))),
            summary_missing: Some(format!("{} is not configured", title_case(&plugin_id))),
        }),
        set: Some(RunSpec {
            accepts_args: false,
            human_success: Some(format!("{} reconciled", title_case(&plugin_id))),
        }),
        go: Some(RunSpec {
            accepts_args: true,
            human_success: Some(format!("{} completed", title_case(&plugin_id))),
        }),
    };
    Ok(Blueprint {
        schema_version: 1,
        plugin: PluginSpec {
            name: plugin_id.clone(),
            crate_name: Some(format!("ready-set-{plugin_id}")),
            binary_name: Some(format!("ready-set-{plugin_id}")),
            description,
            version: "0.1.0".into(),
            stability: StabilitySpec::Experimental,
            min_dispatcher_version: "0.1.0".into(),
            platforms: default_platforms(),
            project_requirements: Vec::new(),
        },
        capabilities: if matches!(kind, PluginKind::Provider) {
            vec![capability]
        } else {
            Vec::new()
        },
        aliases: Vec::new(),
        config: Some(ConfigSpec {
            section: Some(plugin_id),
            fields: Vec::new(),
        }),
        dependencies: DependencySpec::default(),
        generation: GenerationSpec {
            license: Some("MIT OR Apache-2.0".into()),
            include_readme: Some(true),
            include_contract_tests: Some(true),
            include_github_ci: Some(true),
            include_sidecar_manifest: Some(false),
            sdk_version: Some(DEFAULT_SDK_VERSION.into()),
            sdk_path: None,
        },
    })
}

fn generate_project(blueprint: &Blueprint, out_dir: &Path, force: bool) -> Result<()> {
    blueprint.validate()?;
    ensure_output_dir(out_dir, force)?;

    let src = out_dir.join("src");
    let generated = src.join("generated");
    let handlers = src.join("handlers");
    let tests = out_dir.join("tests");
    std::fs::create_dir_all(&src)?;
    std::fs::create_dir_all(&generated)?;
    std::fs::create_dir_all(&handlers)?;
    if blueprint.generation.include_contract_tests() {
        std::fs::create_dir_all(&tests)?;
    }

    write_file(
        &out_dir.join("Cargo.toml"),
        &render_cargo_toml(blueprint),
        force,
    )?;
    write_file(
        &out_dir.join(BLUEPRINT_FILE),
        &serde_yaml_ng::to_string(blueprint)?,
        force,
    )?;
    write_file(&src.join("main.rs"), &render_main_rs(blueprint), force)?;
    write_file(&generated.join("mod.rs"), render_generated_mod_rs(), force)?;
    write_file(
        &generated.join("describe.rs"),
        &render_describe_rs(blueprint),
        force,
    )?;
    write_file(
        &generated.join("config.rs"),
        &render_config_rs(blueprint),
        force,
    )?;
    write_file(
        &generated.join("routing.rs"),
        &render_routing_rs(blueprint),
        force,
    )?;
    write_user_file_if_missing(&handlers.join("mod.rs"), render_handlers_mod_rs())?;
    write_user_file_if_missing(&handlers.join("ready.rs"), render_ready_rs())?;
    write_user_file_if_missing(&handlers.join("set.rs"), render_set_rs())?;
    write_user_file_if_missing(&handlers.join("go.rs"), render_go_rs())?;

    if blueprint.generation.include_readme() {
        write_file(&out_dir.join("README.md"), &render_readme(blueprint), force)?;
    }
    if blueprint.generation.include_contract_tests() {
        write_file(
            &tests.join("contract.rs"),
            &render_contract_test(blueprint),
            force,
        )?;
    }
    if blueprint.generation.include_github_ci() {
        let workflow_dir = out_dir.join(".github/workflows");
        std::fs::create_dir_all(&workflow_dir)?;
        write_file(&workflow_dir.join("ci.yml"), render_github_ci(), force)?;
    }
    if blueprint.generation.include_sidecar_manifest() {
        let dist = out_dir.join("dist");
        std::fs::create_dir_all(&dist)?;
        write_file(
            &dist.join(format!("{}.toml", blueprint.binary_name())),
            &render_sidecar_manifest(blueprint),
            force,
        )?;
    }

    Ok(())
}

fn ensure_output_dir(out_dir: &Path, force: bool) -> Result<()> {
    if !out_dir.exists() {
        std::fs::create_dir_all(out_dir)?;
        return Ok(());
    }
    if !out_dir.is_dir() {
        bail!("{} exists and is not a directory", out_dir.display());
    }
    if !force && out_dir.read_dir()?.next().is_some() {
        bail!(
            "{} already exists and is not empty; pass --force to overwrite generated files",
            out_dir.display()
        );
    }
    Ok(())
}

fn write_file(path: &Path, content: &str, force: bool) -> Result<()> {
    if path.exists() && !force {
        bail!(
            "{} already exists; pass --force to overwrite",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn write_user_file_if_missing(path: &Path, content: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

fn verify_project(out_dir: &Path) -> Result<()> {
    run_check(out_dir, "cargo", &["fmt"])?;
    run_check(out_dir, "cargo", &["fmt", "--check"])?;
    run_check(out_dir, "cargo", &["test"])?;
    Ok(())
}

fn format_project(out_dir: &Path) -> Result<()> {
    run_check(out_dir, "cargo", &["fmt"])
}

fn run_check(dir: &Path, program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .with_context(|| format!("failed to run {program} {}", args.join(" ")))?;
    if !status.success() {
        bail!("{program} {} failed with {status}", args.join(" "));
    }
    Ok(())
}

fn render_cargo_toml(blueprint: &Blueprint) -> String {
    let license = blueprint
        .generation
        .license
        .as_deref()
        .unwrap_or("MIT OR Apache-2.0");
    let sdk_dependency = blueprint.generation.sdk_path.as_ref().map_or_else(
        || toml_string(blueprint.generation.sdk_version()),
        |path| {
            format!(
                "{{ path = {}, version = {} }}",
                toml_string(path),
                toml_string(blueprint.generation.sdk_version())
            )
        },
    );
    format!(
        r#"[package]
name = "{crate_name}"
version = "{version}"
edition = "2024"
rust-version = "1.95"
license = "{license}"
description = "{description}"

[[bin]]
name = "{binary_name}"
path = "src/main.rs"

[dependencies]
ready-set-sdk = {sdk_dependency}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
"#,
        crate_name = blueprint.crate_name(),
        version = blueprint.plugin.version,
        license = escape_toml_basic(license),
        description = escape_toml_basic(&blueprint.plugin.description),
        binary_name = blueprint.binary_name(),
        sdk_dependency = sdk_dependency,
    )
}

fn render_main_rs(blueprint: &Blueprint) -> String {
    let _ = blueprint;
    r"//! Entry point for the generated ready-set plugin.
//!
//! This file is generated by ready-set-plugin. Put plugin behavior in
//! `src/handlers/`; regeneration preserves those files.

mod generated;
mod handlers;

fn main() -> std::process::ExitCode {
    generated::routing::main()
}
"
    .into()
}

const fn render_generated_mod_rs() -> &'static str {
    r"//! Generated contract and routing code.
//!
//! Edit `ready-set-plugin.yaml` and regenerate instead of editing this module.

pub mod config;
pub mod describe;
pub mod routing;
"
}

const fn render_handlers_mod_rs() -> &'static str {
    r"//! User-owned plugin handlers.
//!
//! ready-set-plugin creates these files once and preserves them on
//! regeneration, even with `--force`.

pub mod go;
pub mod ready;
pub mod set;
"
}

#[allow(clippy::too_many_lines)]
fn render_routing_rs(blueprint: &Blueprint) -> String {
    let capabilities = blueprint
        .capabilities
        .iter()
        .map(render_capability_meta)
        .collect::<Vec<_>>()
        .join("\n");
    let command_message = if blueprint.capabilities.is_empty() {
        format!("{} is installed", blueprint.binary_name())
    } else {
        format!(
            "{} is a ready-set provider plugin; use ready-set ready/set/go",
            blueprint.binary_name()
        )
    };
    let command_json = format!(
        r#"serde_json::json!({{"plugin": {}, "status": "ok"}})"#,
        rust_string(&blueprint.plugin.name)
    );

    format!(
        r#"//! Generated plugin routing.
//!
//! Edit handler files under `src/handlers/` for plugin-specific behavior.

use std::ffi::OsString;

use ready_set_sdk::prelude::*;

use crate::generated::describe;
use crate::handlers;

#[derive(Debug, Clone, Copy)]
pub struct CapabilityMeta {{
    pub id: &'static str,
    pub title: &'static str,
    pub relevance: CapabilityRelevance,
    pub supports_ready: bool,
    pub supports_set: bool,
    pub supports_go: bool,
    pub ready_summary: &'static str,
    pub set_accepts_args: bool,
    pub set_human_success: &'static str,
    pub go_accepts_args: bool,
    pub go_human_success: &'static str,
}}

pub const CAPABILITIES: &[CapabilityMeta] = &[
{capabilities}];

pub fn main() -> std::process::ExitCode {{
    let description = describe::describe();
    if let Some(code) = description.handle_arg0_describe(std::env::args_os()) {{
        return code.into();
    }}

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_lifecycle_request(std::env::args_os()) {{
        Ok(Some(request)) => request,
        Ok(None) => return run_user_command(&Context::from_env(), &args).into(),
        Err(err) => {{
            eprintln!("{{}}: {{err}}", describe::BINARY_NAME);
            return ExitCode::UserError.into();
        }},
    }};

    let ctx = Context::from_env();
    run_lifecycle(&ctx, request).into()
}}

fn run_user_command(ctx: &Context, args: &[OsString]) -> ExitCode {{
    if args.first().and_then(|arg| arg.to_str()) == Some("--help") {{
        println!("{{}}", describe::DESCRIPTION);
        return ExitCode::Ok;
    }}

    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {{
        match out.json(&{command_json}) {{
            Ok(()) => ExitCode::Ok,
            Err(err) => {{
                eprintln!("{{}}: {{err}}", describe::BINARY_NAME);
                (&err).into()
            }},
        }}
    }} else {{
        out.human({command_message});
        ExitCode::Ok
    }}
}}

fn run_lifecycle(ctx: &Context, request: LifecycleRequest) -> ExitCode {{
    match request {{
        LifecycleRequest::Ready {{ capability }} => {{
            let Some(meta) = find_capability(capability.as_str()) else {{
                return unknown_capability(capability.as_str());
            }};
            if meta.supports_ready {{
                handlers::ready::run(ctx, meta)
            }} else {{
                unsupported("ready", meta)
            }}
        }},
        LifecycleRequest::Set {{ capability, args }} => {{
            let Some(meta) = find_capability(capability.as_str()) else {{
                return unknown_capability(capability.as_str());
            }};
            if !meta.supports_set {{
                return unsupported("set", meta);
            }}
            if !meta.set_accepts_args && !args.is_empty() {{
                eprintln!("{{}}: `{{}}` set does not accept arguments", describe::BINARY_NAME, meta.id);
                return ExitCode::UserError;
            }}
            handlers::set::run(ctx, meta, &args)
        }},
        LifecycleRequest::Go {{ capability, args }} => {{
            let Some(meta) = find_capability(capability.as_str()) else {{
                return unknown_capability(capability.as_str());
            }};
            if !meta.supports_go {{
                return unsupported("go", meta);
            }}
            if !meta.go_accepts_args && !args.is_empty() {{
                eprintln!("{{}}: `{{}}` go does not accept arguments", describe::BINARY_NAME, meta.id);
                return ExitCode::UserError;
            }}
            handlers::go::run(ctx, meta, &args)
        }},
    }}
}}

fn find_capability(id: &str) -> Option<&'static CapabilityMeta> {{
    CAPABILITIES.iter().find(|capability| capability.id == id)
}}

fn unknown_capability(capability: &str) -> ExitCode {{
    eprintln!("{{}}: unknown capability `{{capability}}`", describe::BINARY_NAME);
    ExitCode::UserError
}}

fn unsupported(verb: &str, capability: &CapabilityMeta) -> ExitCode {{
    eprintln!(
        "{{}}: capability `{{}}` does not support {{verb}}",
        describe::BINARY_NAME,
        capability.id
    );
    ExitCode::UserError
}}
"#,
        capabilities = capabilities,
        command_json = command_json,
        command_message = rust_string(&command_message),
    )
}

fn render_capability_meta(cap: &CapabilitySpec) -> String {
    let ready_summary = cap
        .ready
        .as_ref()
        .and_then(|ready| ready.summary_ready.as_deref())
        .map_or_else(|| format!("{} is configured", cap.title), str::to_owned);
    let set_human_success = cap
        .set
        .as_ref()
        .and_then(|set| set.human_success.as_deref())
        .map_or_else(|| format!("{} set", cap.title), str::to_owned);
    let go_human_success = cap
        .go
        .as_ref()
        .and_then(|go| go.human_success.as_deref())
        .map_or_else(|| format!("{} go", cap.title), str::to_owned);
    format!(
        "    CapabilityMeta {{\n        id: {id},\n        title: {title},\n        relevance: CapabilityRelevance::{relevance},\n        supports_ready: {supports_ready},\n        supports_set: {supports_set},\n        supports_go: {supports_go},\n        ready_summary: {ready_summary},\n        set_accepts_args: {set_accepts_args},\n        set_human_success: {set_human_success},\n        go_accepts_args: {go_accepts_args},\n        go_human_success: {go_human_success},\n    }},",
        id = rust_string(&cap.id),
        title = rust_string(&cap.title),
        relevance = cap.default_relevance.rust_variant(),
        supports_ready = cap.verbs.contains(&VerbSpec::Ready),
        supports_set = cap.verbs.contains(&VerbSpec::Set),
        supports_go = cap.verbs.contains(&VerbSpec::Go),
        ready_summary = rust_string(&ready_summary),
        set_accepts_args = cap.set.as_ref().is_some_and(|set| set.accepts_args),
        set_human_success = rust_string(&set_human_success),
        go_accepts_args = cap.go.as_ref().is_some_and(|go| go.accepts_args),
        go_human_success = rust_string(&go_human_success),
    )
}

fn render_describe_rs(blueprint: &Blueprint) -> String {
    let capabilities = blueprint
        .capabilities
        .iter()
        .map(render_capability_descriptor)
        .collect::<Vec<_>>()
        .join("\n");
    let aliases = blueprint
        .aliases
        .iter()
        .map(render_command_alias)
        .collect::<Vec<_>>()
        .join("\n");
    let platforms = blueprint
        .plugin
        .platforms
        .iter()
        .map(|p| format!("Platform::{}", p.rust_variant()))
        .collect::<Vec<_>>()
        .join(", ");
    let project_requirements = vec_expr(&blueprint.plugin.project_requirements);
    format!(
        r#"//! Static plugin metadata.

use ready_set_sdk::describe::{{Describe, Platform, Stability}};
use ready_set_sdk::prelude::*;

pub const PROVIDER_ID: &str = {provider_id};
pub const BINARY_NAME: &str = {binary_name};
pub const DESCRIPTION: &str = {description};

pub fn describe() -> Describe {{
    Describe {{
        description: DESCRIPTION.into(),
        version: {version}.parse().expect("generated semver should parse"),
        stability: Stability::{stability},
        min_dispatcher_version: {min_dispatcher_version}
            .parse()
            .expect("generated semver should parse"),
        platforms: vec![{platforms}],
        project_requirements: {project_requirements},
        capabilities: vec![
{capabilities}        ],
        command_aliases: vec![
{aliases}        ],
    }}
}}
"#,
        provider_id = rust_string(&blueprint.plugin.name),
        binary_name = rust_string(&blueprint.binary_name()),
        description = rust_string(&blueprint.plugin.description),
        version = rust_string(&blueprint.plugin.version),
        stability = blueprint.plugin.stability.rust_variant(),
        min_dispatcher_version = rust_string(&blueprint.plugin.min_dispatcher_version),
        platforms = platforms,
        project_requirements = project_requirements,
        capabilities = capabilities,
        aliases = aliases,
    )
}

fn render_capability_descriptor(cap: &CapabilitySpec) -> String {
    let verbs = cap
        .verbs
        .iter()
        .map(|v| format!("CapabilityVerb::{}", v.rust_variant()))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "            CapabilityDescriptor {{\n                id: {id}.into(),\n                title: {title}.into(),\n                provider: ProviderId::from(PROVIDER_ID),\n                verbs: vec![{verbs}],\n                default_relevance: CapabilityRelevance::{relevance},\n            }},",
        id = rust_string(&cap.id),
        title = rust_string(&cap.title),
        verbs = verbs,
        relevance = cap.default_relevance.rust_variant(),
    )
}

fn render_command_alias(alias: &AliasSpec) -> String {
    let match_first_arg = option_string_expr(alias.match_first_arg.as_deref());
    let target = match alias.target {
        AliasTargetSpec::Set => format!(
            "CommandAliasTarget::Set {{ capability: {}.into() }}",
            rust_string(alias.capability.as_deref().unwrap_or_default())
        ),
        AliasTargetSpec::Go => format!(
            "CommandAliasTarget::Go {{ capability: {}.into() }}",
            rust_string(alias.capability.as_deref().unwrap_or_default())
        ),
        AliasTargetSpec::Plugin => format!(
            "CommandAliasTarget::Plugin {{ args: {} }}",
            vec_expr(&alias.args)
        ),
    };
    format!(
        "            CommandAlias {{\n                name: {name}.into(),\n                description: {description}.into(),\n                match_first_arg: {match_first_arg},\n                target: {target},\n            }},",
        name = rust_string(&alias.name),
        description = rust_string(&alias.description),
        match_first_arg = match_first_arg,
        target = target,
    )
}

fn render_config_rs(blueprint: &Blueprint) -> String {
    let Some(config) = &blueprint.config else {
        return r"//! Project-local plugin configuration.

use ready_set_sdk::prelude::*;

#[derive(Debug, Clone, Default)]
pub struct PluginConfig {}

pub fn load(_ctx: &Context) -> PluginConfig {
    PluginConfig {}
}
"
        .into();
    };
    if config.fields.is_empty() {
        return r"//! Project-local plugin configuration.

use ready_set_sdk::prelude::*;

#[derive(Debug, Clone, Default)]
pub struct PluginConfig {}

pub fn load(_ctx: &Context) -> PluginConfig {
    PluginConfig {}
}
"
        .into();
    }

    let section = config
        .section
        .as_deref()
        .unwrap_or(blueprint.plugin.name.as_str());
    let mut struct_fields = String::new();
    let mut defaults = String::new();
    let mut loaders = String::new();
    let helpers = render_config_helpers(&config.fields);
    for field in &config.fields {
        let ident = rust_ident(&field.name);
        let ty = field.field_type.rust_type();
        let default_expr = field.default_expr();
        writeln!(struct_fields, "    pub {ident}: {ty},").unwrap();
        writeln!(defaults, "            {ident}: {default_expr},").unwrap();
        writeln!(
            loaders,
            "        cfg.{ident} = read_{reader}(section, {key}).unwrap_or(cfg.{ident});",
            reader = field.field_type.reader_name(),
            key = rust_string(&field.name)
        )
        .unwrap();
    }

    format!(
        r"//! Project-local plugin configuration.

use ready_set_sdk::prelude::*;

#[derive(Debug, Clone)]
pub struct PluginConfig {{
{struct_fields}}}

impl Default for PluginConfig {{
    fn default() -> Self {{
        Self {{
{defaults}        }}
    }}
}}

pub fn load(ctx: &Context) -> PluginConfig {{
    let mut cfg = PluginConfig::default();
    let Some(path) = ctx.config_path() else {{
        return cfg;
    }};
    let Ok(project_cfg) = ready_set_sdk::config::parse_at(path) else {{
        return cfg;
    }};
    let Some(section) = project_cfg.plugins.get({section}) else {{
        return cfg;
    }};
{loaders}    cfg
}}

{helpers}",
        struct_fields = struct_fields,
        defaults = defaults,
        section = rust_string(section),
        loaders = loaders,
        helpers = helpers,
    )
}

fn render_config_helpers(fields: &[ConfigField]) -> String {
    let mut helpers = String::new();
    if fields
        .iter()
        .any(|field| matches!(field.field_type, ConfigFieldType::Bool))
    {
        helpers.push_str(
            r"fn read_bool(section: &ready_set_sdk::config::PluginSection, key: &str) -> Option<bool> {
    section.get_bool(key)
}

",
        );
    }
    if fields
        .iter()
        .any(|field| matches!(field.field_type, ConfigFieldType::String))
    {
        helpers.push_str(
            r"fn read_string(section: &ready_set_sdk::config::PluginSection, key: &str) -> Option<String> {
    section.get_str(key).map(str::to_owned)
}

",
        );
    }
    if fields
        .iter()
        .any(|field| matches!(field.field_type, ConfigFieldType::Integer))
    {
        helpers.push_str(
            r"fn read_integer(section: &ready_set_sdk::config::PluginSection, key: &str) -> Option<i64> {
    section.get_integer(key)
}

",
        );
    }
    if fields
        .iter()
        .any(|field| matches!(field.field_type, ConfigFieldType::ArrayString))
    {
        helpers.push_str(
            r"fn read_array_string(section: &ready_set_sdk::config::PluginSection, key: &str) -> Option<Vec<String>> {
    section.get_string_array(key)
}
",
        );
    }
    helpers
}

const fn render_ready_rs() -> &'static str {
    r#"//! User-owned readiness handler.
//!
//! This file is created once. Regeneration preserves it.

use ready_set_sdk::prelude::*;

use crate::generated::config;
use crate::generated::describe;
use crate::generated::routing::CapabilityMeta;

pub fn run(ctx: &Context, capability: &CapabilityMeta) -> ExitCode {
    let _cfg = config::load(ctx);
    let report = CapabilityReport {
        id: capability.id.into(),
        title: capability.title.into(),
        provider: ProviderId::from(describe::PROVIDER_ID),
        state: CapabilityState::Ready,
        relevance: capability.relevance,
        summary: capability.ready_summary.into(),
        next_action: next_action(capability),
    };
    emit_json(ctx, &report)
}

fn next_action(capability: &CapabilityMeta) -> Option<NextAction> {
    if capability.supports_go {
        Some(NextAction {
            command: format!("ready-set go {}", capability.id),
            description: format!("Run {}", capability.title),
        })
    } else if capability.supports_set {
        Some(NextAction {
            command: format!("ready-set set {}", capability.id),
            description: format!("Reconcile {}", capability.title),
        })
    } else {
        None
    }
}

fn emit_json<T: serde::Serialize>(ctx: &Context, value: &T) -> ExitCode {
    let mut out = Output::for_context(ctx, std::io::stdout());
    match out.json(value) {
        Ok(()) => ExitCode::Ok,
        Err(err) => {
            eprintln!("{}: {err}", describe::BINARY_NAME);
            (&err).into()
        },
    }
}
"#
}

const fn render_set_rs() -> &'static str {
    r#"//! User-owned set handler.
//!
//! This file is created once. Regeneration preserves it.

use std::ffi::OsString;

use ready_set_sdk::prelude::*;

use crate::generated::config;
use crate::generated::describe;
use crate::generated::routing::CapabilityMeta;

pub fn run(ctx: &Context, capability: &CapabilityMeta, args: &[OsString]) -> ExitCode {
    let _ = args;
    let _cfg = config::load(ctx);
    let report = CapabilityRunReport {
        id: capability.id.into(),
        verb: CapabilityVerb::Set,
        status: RunStatus::Noop,
        actions: vec![CapabilityAction {
            kind: CapabilityActionKind::Check,
            summary: capability.set_human_success.into(),
            path: None,
        }],
    };
    emit_run(ctx, &report, capability.set_human_success)
}

fn emit_run(ctx: &Context, report: &CapabilityRunReport, human_success: &str) -> ExitCode {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        match out.json(report) {
            Ok(()) => ExitCode::Ok,
            Err(err) => {
                eprintln!("{}: {err}", describe::BINARY_NAME);
                (&err).into()
            },
        }
    } else {
        out.human(human_success);
        ExitCode::Ok
    }
}
"#
}

const fn render_go_rs() -> &'static str {
    r#"//! User-owned go handler.
//!
//! This file is created once. Regeneration preserves it.

use std::ffi::OsString;

use ready_set_sdk::prelude::*;

use crate::generated::config;
use crate::generated::describe;
use crate::generated::routing::CapabilityMeta;

pub fn run(ctx: &Context, capability: &CapabilityMeta, args: &[OsString]) -> ExitCode {
    let _ = args;
    let _cfg = config::load(ctx);
    let report = CapabilityRunReport {
        id: capability.id.into(),
        verb: CapabilityVerb::Go,
        status: RunStatus::Noop,
        actions: vec![CapabilityAction {
            kind: CapabilityActionKind::Run,
            summary: capability.go_human_success.into(),
            path: None,
        }],
    };
    emit_run(ctx, &report, capability.go_human_success)
}

fn emit_run(ctx: &Context, report: &CapabilityRunReport, human_success: &str) -> ExitCode {
    let mut out = Output::for_context(ctx, std::io::stdout());
    if matches!(ctx.output_mode(), OutputMode::Json) {
        match out.json(report) {
            Ok(()) => ExitCode::Ok,
            Err(err) => {
                eprintln!("{}: {err}", describe::BINARY_NAME);
                (&err).into()
            },
        }
    } else {
        out.human(human_success);
        ExitCode::Ok
    }
}
"#
}

fn render_readme(blueprint: &Blueprint) -> String {
    let capability_lines = if blueprint.capabilities.is_empty() {
        "This plugin declares no lifecycle capabilities. It runs as a direct command.\n".into()
    } else {
        blueprint
            .capabilities
            .iter()
            .fold(String::new(), |mut acc, cap| {
                let verbs = cap
                    .verbs
                    .iter()
                    .map(VerbSpec::wire_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(acc, "- `{}`: {} ({verbs})", cap.id, cap.title);
                acc
            })
    };
    let direct_examples = if blueprint.capabilities.is_empty() {
        format!(
            "{} --help\n{} --json\n",
            blueprint.binary_name(),
            blueprint.binary_name()
        )
    } else {
        blueprint
            .capabilities
            .iter()
            .flat_map(|cap| {
                cap.verbs.iter().map(move |verb| {
                    format!(
                        "{} __{} {}\n",
                        blueprint.binary_name(),
                        verb.wire_name(),
                        cap.id
                    )
                })
            })
            .collect::<String>()
    };
    format!(
        r"# {crate_name}

{description}

This crate was generated from `{blueprint_file}`. The generated binary is
`{binary_name}`, so the dispatcher invokes it as:

```text
ready-set {plugin_name} ...
```

## Capabilities

{capability_lines}
## Local Checks

```text
cargo test
cargo run -- __describe
{direct_examples}```

## Where To Put Your Logic

- `src/handlers/ready.rs` implements read-only readiness reports.
- `src/handlers/set.rs` implements reconciliation handlers.
- `src/handlers/go.rs` implements workflow handlers.
- `src/generated/config.rs` loads this plugin's `.ready-set.toml` section.
- `src/generated/describe.rs` owns static metadata.
- `src/generated/routing.rs` routes lifecycle requests to your handlers.

Files under `src/generated/` are overwritten by regeneration. Files under
`src/handlers/` are created once and preserved, even with `--force`.

Regenerate by editing `{blueprint_file}` and running:

```text
ready-set plugin validate {blueprint_file}
ready-set plugin generate {blueprint_file} --path . --force
```
",
        crate_name = blueprint.crate_name(),
        description = blueprint.plugin.description,
        blueprint_file = BLUEPRINT_FILE,
        binary_name = blueprint.binary_name(),
        plugin_name = blueprint.plugin.name,
        capability_lines = capability_lines,
        direct_examples = direct_examples,
    )
}

fn render_contract_test(blueprint: &Blueprint) -> String {
    let first_cap = blueprint.capabilities.first();
    let ready_test = first_cap.map_or_else(String::new, |cap| {
        format!(
            r#"
#[test]
fn ready_protocol_returns_json_report() {{
    let out = Command::new(plugin())
        .args(["__ready", {cap_id}])
        .env("READY_SET_OUTPUT", "json")
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {{}}", String::from_utf8_lossy(&out.stderr));
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(parsed["id"], {cap_id});
    assert_eq!(parsed["provider"], {provider_id});
}}
"#,
            cap_id = rust_string(&cap.id),
            provider_id = rust_string(&blueprint.plugin.name),
        )
    });
    format!(
        r#"use std::process::Command;

fn plugin() -> &'static str {{
    env!("CARGO_BIN_EXE_{binary_name}")
}}

#[test]
fn describe_emits_contract_metadata() {{
    let out = Command::new(plugin()).arg("__describe").output().unwrap();
    assert!(out.status.success(), "stderr: {{}}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(parsed["description"], {description});
    assert_eq!(parsed["capabilities"].as_array().unwrap().len(), {capability_count});
    assert_eq!(
        parsed
            .get("project_requirements")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or(0),
        {requirement_count}
    );
}}
{ready_test}
"#,
        binary_name = blueprint.binary_name(),
        description = rust_string(&blueprint.plugin.description),
        capability_count = blueprint.capabilities.len(),
        requirement_count = blueprint.plugin.project_requirements.len(),
        ready_test = ready_test,
    )
}

const fn render_github_ci() -> &'static str {
    r"name: CI

on:
  push:
  pull_request:

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo fmt --check
      - run: cargo test
"
}

fn render_sidecar_manifest(blueprint: &Blueprint) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "description = {}\nversion = {}\nstability = {}\nmin_dispatcher_version = {}",
        toml_string(&blueprint.plugin.description),
        toml_string(&blueprint.plugin.version),
        toml_string(blueprint.plugin.stability.wire_name()),
        toml_string(&blueprint.plugin.min_dispatcher_version)
    )
    .unwrap();
    writeln!(
        out,
        "platforms = [{}]",
        blueprint
            .plugin
            .platforms
            .iter()
            .map(|p| toml_string(p.wire_name()))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();
    if !blueprint.plugin.project_requirements.is_empty() {
        writeln!(
            out,
            "project_requirements = [{}]",
            blueprint
                .plugin
                .project_requirements
                .iter()
                .map(|r| toml_string(r))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    for cap in &blueprint.capabilities {
        writeln!(
            out,
            "\n[[capabilities]]\nid = {}\ntitle = {}\nprovider = {}\nverbs = [{}]\ndefault_relevance = {}",
            toml_string(&cap.id),
            toml_string(&cap.title),
            toml_string(&blueprint.plugin.name),
            cap.verbs
                .iter()
                .map(|v| toml_string(v.wire_name()))
                .collect::<Vec<_>>()
                .join(", "),
            toml_string(cap.default_relevance.wire_name())
        )
        .unwrap();
    }
    for alias in &blueprint.aliases {
        writeln!(
            out,
            "\n[[command_aliases]]\nname = {}\ndescription = {}\ntarget = {}",
            toml_string(&alias.name),
            toml_string(&alias.description),
            toml_string(alias.target.wire_name())
        )
        .unwrap();
        if let Some(match_first_arg) = &alias.match_first_arg {
            writeln!(out, "match_first_arg = {}", toml_string(match_first_arg)).unwrap();
        }
        if let Some(capability) = &alias.capability {
            writeln!(out, "capability = {}", toml_string(capability)).unwrap();
        }
        if !alias.args.is_empty() {
            writeln!(
                out,
                "args = [{}]",
                alias
                    .args
                    .iter()
                    .map(|arg| toml_string(arg))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
            .unwrap();
        }
    }
    out
}

impl Blueprint {
    #[allow(clippy::too_many_lines)]
    fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            bail!(
                "unsupported plugin blueprint schema_version {}; expected 1",
                self.schema_version
            );
        }
        validate_plugin_name(&self.plugin.name)?;
        validate_kebab("crate name", &self.crate_name())?;
        validate_kebab("binary name", &self.binary_name())?;
        if !self.binary_name().starts_with("ready-set-") {
            bail!("binary_name must start with ready-set-");
        }
        semver::Version::parse(&self.plugin.version)
            .with_context(|| "plugin.version must be valid semver")?;
        semver::Version::parse(&self.plugin.min_dispatcher_version)
            .with_context(|| "plugin.min_dispatcher_version must be valid semver")?;
        if self.plugin.description.is_empty() || self.plugin.description.len() > 80 {
            bail!("plugin.description must be 1-80 characters");
        }
        if self.plugin.platforms.is_empty() {
            bail!("plugin.platforms must not be empty");
        }
        validate_unique(
            "plugin platform",
            self.plugin
                .platforms
                .iter()
                .map(|platform| platform.wire_name()),
        )?;
        for requirement in &self.plugin.project_requirements {
            if requirement.is_empty() || requirement.chars().any(char::is_whitespace) {
                bail!("project requirement `{requirement}` must be nonempty with no whitespace");
            }
        }
        validate_unique(
            "project requirement",
            self.plugin.project_requirements.iter().map(String::as_str),
        )?;
        validate_unique(
            "capability id",
            self.capabilities.iter().map(|cap| cap.id.as_str()),
        )?;
        for cap in &self.capabilities {
            validate_kebab("capability id", &cap.id)?;
            if cap.title.is_empty() {
                bail!("capability `{}` title must not be empty", cap.id);
            }
            if cap.verbs.is_empty() {
                bail!("capability `{}` must declare at least one verb", cap.id);
            }
            validate_unique("capability verb", cap.verbs.iter().map(VerbSpec::wire_name))?;
            if let Some(ready) = &cap.ready {
                if ready.summary_ready.as_deref().is_some_and(str::is_empty) {
                    bail!(
                        "capability `{}` ready.summary_ready must not be empty",
                        cap.id
                    );
                }
                if ready.summary_missing.as_deref().is_some_and(str::is_empty) {
                    bail!(
                        "capability `{}` ready.summary_missing must not be empty",
                        cap.id
                    );
                }
            }
            if cap
                .set
                .as_ref()
                .and_then(|set| set.human_success.as_deref())
                .is_some_and(str::is_empty)
            {
                bail!(
                    "capability `{}` set.human_success must not be empty",
                    cap.id
                );
            }
            if cap
                .go
                .as_ref()
                .and_then(|go| go.human_success.as_deref())
                .is_some_and(str::is_empty)
            {
                bail!("capability `{}` go.human_success must not be empty", cap.id);
            }
        }
        validate_unique(
            "alias name",
            self.aliases.iter().map(|alias| alias.name.as_str()),
        )?;
        for alias in &self.aliases {
            validate_kebab("alias name", &alias.name)?;
            if alias.description.is_empty() || alias.description.len() > 80 {
                bail!("alias `{}` description must be 1-80 characters", alias.name);
            }
            if matches!(alias.target, AliasTargetSpec::Set | AliasTargetSpec::Go)
                && alias.capability.is_none()
            {
                bail!("alias `{}` target requires capability", alias.name);
            }
            if matches!(alias.target, AliasTargetSpec::Plugin) && alias.capability.is_some() {
                bail!(
                    "alias `{}` plugin target must not set capability",
                    alias.name
                );
            }
            if let Some(capability) = &alias.capability {
                validate_kebab("alias capability", capability)?;
                if !self.capabilities.iter().any(|cap| cap.id == *capability) {
                    bail!(
                        "alias `{}` references unknown capability `{capability}`",
                        alias.name
                    );
                }
            }
            if let Some(match_first_arg) = &alias.match_first_arg
                && match_first_arg.is_empty()
            {
                bail!("alias `{}` match_first_arg must not be empty", alias.name);
            }
        }
        if let Some(config) = &self.config {
            if let Some(section) = &config.section {
                validate_kebab("config section", section)?;
            }
            let mut field_idents = HashSet::new();
            for field in &config.fields {
                if field.name.is_empty() {
                    bail!("config field name must not be empty");
                }
                if !field
                    .name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
                {
                    bail!(
                        "config field `{}` must use only ASCII letters, digits, hyphen, or underscore",
                        field.name
                    );
                }
                let ident = rust_ident(&field.name);
                if !field_idents.insert(ident) {
                    bail!(
                        "config field `{}` collides with another field after Rust identifier normalization",
                        field.name
                    );
                }
                field.validate_default()?;
            }
        }
        for tool in &self.dependencies.external_tools {
            if tool.is_empty() {
                bail!("dependency external_tools entries must not be empty");
            }
        }
        if self
            .generation
            .license
            .as_deref()
            .is_some_and(str::is_empty)
        {
            bail!("generation.license must not be empty");
        }
        if self
            .generation
            .sdk_version
            .as_deref()
            .is_some_and(str::is_empty)
        {
            bail!("generation.sdk_version must not be empty");
        }
        if self
            .generation
            .sdk_path
            .as_deref()
            .is_some_and(str::is_empty)
        {
            bail!("generation.sdk_path must not be empty");
        }
        Ok(())
    }

    fn crate_name(&self) -> String {
        self.plugin
            .crate_name
            .clone()
            .unwrap_or_else(|| format!("ready-set-{}", self.plugin.name))
    }

    fn binary_name(&self) -> String {
        self.plugin
            .binary_name
            .clone()
            .unwrap_or_else(|| self.crate_name())
    }
}

impl ConfigField {
    fn validate_default(&self) -> Result<()> {
        let Some(default) = &self.default else {
            return Ok(());
        };
        let ok = match self.field_type {
            ConfigFieldType::Bool => default.as_bool().is_some(),
            ConfigFieldType::String => default.as_str().is_some(),
            ConfigFieldType::Integer => default.as_i64().is_some(),
            ConfigFieldType::ArrayString => default
                .as_sequence()
                .is_some_and(|items| items.iter().all(|item| item.as_str().is_some())),
        };
        if ok {
            Ok(())
        } else {
            bail!(
                "default for config field `{}` does not match type",
                self.name
            );
        }
    }

    fn default_expr(&self) -> String {
        match self.field_type {
            ConfigFieldType::Bool => self
                .default
                .as_ref()
                .and_then(serde_yaml_ng::Value::as_bool)
                .unwrap_or(false)
                .to_string(),
            ConfigFieldType::String => {
                let value = self
                    .default
                    .as_ref()
                    .and_then(serde_yaml_ng::Value::as_str)
                    .unwrap_or_default();
                format!("{}.into()", rust_string(value))
            },
            ConfigFieldType::Integer => self
                .default
                .as_ref()
                .and_then(serde_yaml_ng::Value::as_i64)
                .unwrap_or(0)
                .to_string(),
            ConfigFieldType::ArrayString => {
                let values = self
                    .default
                    .as_ref()
                    .and_then(serde_yaml_ng::Value::as_sequence)
                    .into_iter()
                    .flatten()
                    .filter_map(serde_yaml_ng::Value::as_str)
                    .map(|v| format!("{}.into()", rust_string(v)))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("vec![{values}]")
            },
        }
    }
}

impl StabilitySpec {
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::Stable => "Stable",
            Self::Experimental => "Experimental",
            Self::Deprecated => "Deprecated",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Experimental => "experimental",
            Self::Deprecated => "deprecated",
        }
    }
}

impl PlatformSpec {
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Macos => "Macos",
            Self::Windows => "Windows",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        }
    }
}

impl RelevanceSpec {
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::Required => "Required",
            Self::Optional => "Optional",
            Self::NotNeeded => "NotNeeded",
        }
    }

    const fn wire_name(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::NotNeeded => "not-needed",
        }
    }
}

impl VerbSpec {
    const fn rust_variant(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Set => "Set",
            Self::Go => "Go",
        }
    }

    // Used as a function pointer in `iter().map(VerbSpec::wire_name)` over
    // borrowed verbs, which requires `&self`.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    const fn wire_name(&self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Set => "set",
            Self::Go => "go",
        }
    }
}

impl AliasTargetSpec {
    const fn wire_name(self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Go => "go",
            Self::Plugin => "plugin",
        }
    }
}

impl ConfigFieldType {
    const fn rust_type(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::String => "String",
            Self::Integer => "i64",
            Self::ArrayString => "Vec<String>",
        }
    }

    const fn reader_name(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::String => "string",
            Self::Integer => "integer",
            Self::ArrayString => "array_string",
        }
    }
}

fn parse_verbs(raw: &str) -> Result<Vec<VerbSpec>> {
    let mut verbs = Vec::new();
    for part in raw.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let verb = match part {
            "ready" => VerbSpec::Ready,
            "set" => VerbSpec::Set,
            "go" => VerbSpec::Go,
            other => bail!("unknown verb `{other}`; expected ready,set,go"),
        };
        if !verbs.contains(&verb) {
            verbs.push(verb);
        }
    }
    if verbs.is_empty() {
        bail!("at least one verb is required");
    }
    Ok(verbs)
}

fn validate_plugin_name(raw: &str) -> Result<()> {
    validate_kebab("plugin name", raw)?;
    if RESERVED_PLUGIN_NAMES.contains(&raw) {
        bail!("plugin name `{raw}` is reserved");
    }
    Ok(())
}

fn validate_kebab(label: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{label} must not be empty");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        bail!("{label} must not be empty");
    };
    if !first.is_ascii_lowercase() {
        bail!("{label} `{value}` must start with a lowercase ASCII letter");
    }
    let mut prev_dash = false;
    for ch in std::iter::once(first).chain(chars) {
        let ok = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-';
        if !ok {
            bail!("{label} `{value}` must be lowercase kebab-case");
        }
        if ch == '-' && prev_dash {
            bail!("{label} `{value}` must not contain consecutive dashes");
        }
        prev_dash = ch == '-';
    }
    if value.ends_with('-') {
        bail!("{label} `{value}` must not end with a dash");
    }
    Ok(())
}

fn validate_unique<'a, I>(label: &str, values: I) -> Result<()>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            bail!("{label} `{value}` must be unique");
        }
    }
    Ok(())
}

fn normalize_plugin_name(raw: &str) -> Result<String> {
    let stripped = raw.strip_prefix("ready-set-").unwrap_or(raw);
    validate_plugin_name(stripped)?;
    Ok(stripped.to_owned())
}

fn rust_ident(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if RUST_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

fn title_case(kebab: &str) -> String {
    kebab
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut out = String::new();
            out.push(first.to_ascii_uppercase());
            out.extend(chars);
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn toml_string(value: &str) -> String {
    format!("\"{}\"", escape_toml_basic(value))
}

fn escape_toml_basic(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn vec_expr(values: &[String]) -> String {
    if values.is_empty() {
        "Vec::new()".into()
    } else {
        format!(
            "vec![{}]",
            values
                .iter()
                .map(|v| format!("{}.into()", rust_string(v)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn option_string_expr(value: Option<&str>) -> String {
    value.map_or_else(
        || "None".into(),
        |v| format!("Some({}.into())", rust_string(v)),
    )
}

fn default_platforms() -> Vec<PlatformSpec> {
    vec![
        PlatformSpec::Linux,
        PlatformSpec::Macos,
        PlatformSpec::Windows,
    ]
}

const fn default_relevance() -> RelevanceSpec {
    RelevanceSpec::Required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starter_blueprint_uses_generic_requirements() {
        let blueprint = starter_blueprint("scan", PluginKind::Provider).unwrap();
        assert_eq!(blueprint.plugin.name, "scan");
        assert_eq!(blueprint.binary_name(), "ready-set-scan");
        assert!(blueprint.plugin.project_requirements.is_empty());
        assert_eq!(blueprint.capabilities[0].id, "scan");
    }

    #[test]
    fn parses_yaml_blueprint_with_config_fields() {
        let raw = r#"
schema_version: 1
plugin:
  name: scan
  description: Scan project files
  version: 0.1.0
  stability: experimental
  min_dispatcher_version: 0.1.0
capabilities:
  - id: policy-scan
    title: Policy Scan
    verbs: [ready, go]
config:
  section: scan
  fields:
    - name: exclude
      type: array_string
      default: ["target/**"]
"#;
        let blueprint: Blueprint = serde_yaml_ng::from_str(raw).unwrap();
        blueprint.validate().unwrap();
        assert_eq!(blueprint.plugin.platforms.len(), 3);
        assert_eq!(blueprint.config.unwrap().fields[0].name, "exclude");
    }

    #[test]
    fn validate_subcommand_parses_blueprint_path() {
        let cli =
            Cli::try_parse_from(["ready-set-plugin", "validate", "ready-set-plugin.yaml"]).unwrap();
        let args = match cli.command {
            Some(Commands::Validate(args)) => args,
            other => unreachable!("expected validate command, got {other:?}"),
        };
        assert_eq!(args.blueprint, PathBuf::from("ready-set-plugin.yaml"));
    }

    #[test]
    fn load_blueprint_rejects_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(BLUEPRINT_FILE);
        std::fs::write(
            &path,
            r"
schema_version: 1
plugin:
  name: scan
  description: Scan project files
  version: 0.1.0
  stability: experimental
  min_dispatcher_version: 0.1.0
unexpected: true
",
        )
        .unwrap();
        let err = load_blueprint(&path).unwrap_err();
        assert!(err.to_string().contains("failed to parse"));
        assert!(format!("{err:#}").contains("unknown field"));
    }

    #[test]
    fn plugin_blueprint_schema_is_valid_json() {
        let schema_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("docs/contracts/schemas/plugin-blueprint.schema.json");
        let raw = std::fs::read_to_string(schema_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            parsed["$schema"],
            "https://json-schema.org/draft/2020-12/schema"
        );
        assert_eq!(parsed["properties"]["schema_version"]["const"], 1);
    }

    #[test]
    fn rejects_reserved_plugin_name() {
        let err = starter_blueprint("ready", PluginKind::Command).unwrap_err();
        assert!(err.to_string().contains("reserved"));
    }

    #[test]
    fn generates_complete_project_files() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("ready-set-scan");
        let blueprint = starter_blueprint("scan", PluginKind::Provider).unwrap();
        generate_project(&blueprint, &out, false).unwrap();
        assert!(out.join("Cargo.toml").is_file());
        assert!(out.join(BLUEPRINT_FILE).is_file());
        assert!(out.join("src/main.rs").is_file());
        assert!(out.join("src/generated/mod.rs").is_file());
        assert!(out.join("src/generated/describe.rs").is_file());
        assert!(out.join("src/generated/config.rs").is_file());
        assert!(out.join("src/generated/routing.rs").is_file());
        assert!(out.join("src/handlers/mod.rs").is_file());
        assert!(out.join("src/handlers/ready.rs").is_file());
        assert!(out.join("src/handlers/set.rs").is_file());
        assert!(out.join("src/handlers/go.rs").is_file());
        assert!(out.join("tests/contract.rs").is_file());
        let cargo_toml = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(!cargo_toml.contains("toml = \"0.8\""));
        let describe = std::fs::read_to_string(out.join("src/generated/describe.rs")).unwrap();
        assert!(describe.contains("project_requirements: Vec::new()"));
    }

    #[test]
    fn force_regeneration_preserves_user_handlers() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("ready-set-scan");
        let mut blueprint = starter_blueprint("scan", PluginKind::Provider).unwrap();
        generate_project(&blueprint, &out, false).unwrap();

        let custom_go = r#"//! User-owned go handler.

use std::ffi::OsString;

use ready_set_sdk::prelude::*;

use crate::generated::routing::CapabilityMeta;

const CUSTOM_MARKER: &str = "custom go logic";

pub fn run(ctx: &Context, capability: &CapabilityMeta, args: &[OsString]) -> ExitCode {
    let _ = (ctx, capability, args, CUSTOM_MARKER);
    ExitCode::Ok
}
"#;
        std::fs::write(out.join("src/handlers/go.rs"), custom_go).unwrap();

        blueprint.plugin.description = "Updated scan provider".into();
        generate_project(&blueprint, &out, true).unwrap();

        let go = std::fs::read_to_string(out.join("src/handlers/go.rs")).unwrap();
        assert!(go.contains("custom go logic"));

        let describe = std::fs::read_to_string(out.join("src/generated/describe.rs")).unwrap();
        assert!(describe.contains("Updated scan provider"));
    }

    #[test]
    fn renders_plugin_alias_args() {
        let mut blueprint = starter_blueprint("scan", PluginKind::Command).unwrap();
        blueprint.aliases = vec![AliasSpec {
            name: "scan".into(),
            description: "Run scan".into(),
            match_first_arg: None,
            target: AliasTargetSpec::Plugin,
            capability: None,
            args: vec!["run".into()],
        }];
        let rendered = render_describe_rs(&blueprint);
        assert!(rendered.contains("CommandAliasTarget::Plugin { args: vec![\"run\".into()] }"));
    }
}
