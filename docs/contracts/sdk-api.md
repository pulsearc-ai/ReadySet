# Contract: `ready-set-sdk` public Rust API

| Field     | Value           |
|-----------|-----------------|
| Stability | `stable`        |
| Version   | `0.1.0`         |
| Crate     | `ready-set-sdk` |

This document is the source of truth for the public API surface of
`ready-set-sdk` at v0.1.0. Every `pub` item listed here is part of the
stable API; items not listed are private. CI gates the SDK against this
document via `cargo public-api`.

The SDK itself follows cargo semver within its own crate version. Adding a
new `pub` item is a minor change; removing or changing the signature of an
existing one is a breaking change.

## Module structure

```text
ready_set_sdk
├── prelude
├── capability
├── context
├── output
├── exit_code
├── change_log
├── describe
├── command_alias
├── manifest
├── sandbox
├── config
├── fs
├── dispatch
├── lifecycle
├── logging
└── error
```

## Crate root

```rust
pub mod context;
pub mod capability;
pub mod output;
pub mod exit_code;
pub mod change_log;
pub mod describe;
pub mod command_alias;
pub mod manifest;
pub mod sandbox;
pub mod config;
pub mod fs;
pub mod dispatch;
pub mod lifecycle;
pub mod logging;
pub mod error;
pub mod prelude;

pub use context::Context;
pub use capability::{
    CapabilityAction, CapabilityActionKind, CapabilityDescriptor, CapabilityId,
    CapabilityRelevance, CapabilityReport, CapabilityRunReport, CapabilityState,
    CapabilityVerb, NextAction, ProviderId, RunStatus,
};
pub use command_alias::{CommandAlias, CommandAliasTarget};
pub use output::{Output, OutputMode};
pub use exit_code::ExitCode;
pub use lifecycle::{
    LifecycleRequest, LifecycleRequestError, parse_lifecycle_request,
};
pub use error::{Error, Result};
```

## `prelude`

```rust
pub use crate::context::Context;
pub use crate::capability::{
    CapabilityAction, CapabilityActionKind, CapabilityDescriptor, CapabilityId,
    CapabilityRelevance, CapabilityReport, CapabilityRunReport, CapabilityState,
    CapabilityVerb, NextAction, ProviderId, RunStatus,
};
pub use crate::command_alias::{CommandAlias, CommandAliasTarget};
pub use crate::exit_code::ExitCode;
pub use crate::lifecycle::{
    LifecycleRequest, LifecycleRequestError, parse_lifecycle_request,
};
pub use crate::output::{Output, OutputMode};
pub use crate::error::{Error, Result};
```

## `capability`

```rust
pub struct CapabilityId { /* fields private */ }
pub struct ProviderId { /* fields private */ }

impl CapabilityId {
    pub fn new(id: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

impl ProviderId {
    pub fn new(id: impl Into<String>) -> Self;
    pub fn as_str(&self) -> &str;
}

pub enum CapabilityVerb { Ready, Set, Go }
pub enum CapabilityState {
    Ready,
    Missing,
    Incomplete,
    Blocked,
    Stale,
    Optional,
    NotNeeded,
}
pub enum CapabilityRelevance { Required, Optional, NotNeeded }

pub struct CapabilityDescriptor {
    pub id: CapabilityId,
    pub title: String,
    pub provider: ProviderId,
    pub verbs: Vec<CapabilityVerb>,
    pub default_relevance: CapabilityRelevance,
}

pub struct NextAction {
    pub command: String,
    pub description: String,
}

pub struct CapabilityReport {
    pub id: CapabilityId,
    pub title: String,
    pub provider: ProviderId,
    pub state: CapabilityState,
    pub relevance: CapabilityRelevance,
    pub summary: String,
    pub next_action: Option<NextAction>,
}

pub enum RunStatus { Ok, Changed, Noop, Failed }
pub enum CapabilityActionKind {
    Create,
    Modify,
    Delete,
    Run,
    Check,
    Skip,
    Error,
}

pub struct CapabilityAction {
    pub kind: CapabilityActionKind,
    pub summary: String,
    pub path: Option<String>,
}

pub struct CapabilityRunReport {
    pub id: CapabilityId,
    pub verb: CapabilityVerb,
    pub status: RunStatus,
    pub actions: Vec<CapabilityAction>,
}
```

`CapabilityRunReport.verb` serializes and deserializes only `set` and `go`.
`ready` is valid for capability descriptors but not for run reports.

## `lifecycle`

```rust
pub enum LifecycleRequest {
    Ready { capability: CapabilityId },
    Set { capability: CapabilityId, args: Vec<std::ffi::OsString> },
    Go { capability: CapabilityId, args: Vec<std::ffi::OsString> },
}

pub struct LifecycleRequestError { /* fields private */ }

impl LifecycleRequestError {
    pub fn message(&self) -> &str;
}

pub fn parse_lifecycle_request(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> std::result::Result<Option<LifecycleRequest>, LifecycleRequestError>;
```

`parse_lifecycle_request` lets provider plugins handle dispatcher protocol
calls such as `__ready`, `__set`, and `__go` before falling back to their
normal user-facing CLI.

## `context`

```rust
pub struct Context { /* fields private */ }

impl Context {
    pub fn from_env() -> Self;
    pub fn dispatcher_version(&self) -> Option<&semver::Version>;
    pub fn project_root(&self) -> Option<&std::path::Path>;
    pub fn config_path(&self) -> Option<&std::path::Path>;
    pub fn output_mode(&self) -> OutputMode;
    pub fn log_level(&self) -> LogLevel;
    pub fn color(&self) -> ColorMode;
    pub fn project_root_or_cwd(&self) -> std::path::PathBuf;
}

pub enum LogLevel { Quiet, Normal, Verbose }
pub enum ColorMode { Auto, Always, Never }
```

## `output`

```rust
pub enum OutputMode { Human, Json }

pub struct Output { /* fields private */ }

impl Output {
    pub fn for_context(ctx: &Context, stdout: std::io::Stdout) -> Self;
    pub fn human(&mut self, msg: &str);
    pub fn json<T: serde::Serialize>(&mut self, value: &T) -> Result<()>;
    pub fn error(&mut self, err: &dyn std::error::Error);
}
```

## `exit_code`

```rust
pub enum ExitCode {
    Ok,
    UserError,
    SystemError,
    DependencyMissing,
    ProjectRequirementMissing,
    ContractViolation,
    UnknownSubcommand,
    Signaled(u8),
}

impl ExitCode {
    /// Numeric exit code as a byte. Unit variants map to the values listed
    /// in `docs/contracts/exit-codes.md` (`Ok = 0`, … `UnknownSubcommand =
    /// 127`); `Signaled(n)` returns `128 + n`, saturating at `255`.
    pub const fn as_u8(self) -> u8;
}

impl From<ExitCode> for std::process::ExitCode { /* ... */ }
impl From<&Error> for ExitCode { /* ... */ }
```

`Signaled(u8)` carries the OS signal number from `ExitStatusExt::signal()`
on Unix; on Windows it is unreachable (children always have
`ExitStatus::code() == Some(_)`). The numeric exit code emitted to the OS
follows the POSIX shell convention `128 + signum`.

## `change_log`

```rust
pub enum ChangeOp { Create, Modify, Delete }

pub struct ChangeRecord {
    pub op: ChangeOp,
    pub path: std::path::PathBuf,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
    pub ts: time::OffsetDateTime,
}

pub struct ChangeLog { /* fields private */ }

impl ChangeLog {
    pub fn open(project_root: &std::path::Path, plugin: &str) -> Result<Self>;
    pub fn record(&mut self, record: ChangeRecord) -> Result<()>;
    pub fn flush(&mut self) -> Result<()>;
}

pub fn reverse_dir(project_root: &std::path::Path)
    -> Result<Vec<(std::path::PathBuf, ChangeRecord)>>;

pub fn backup_file(project_root: &std::path::Path, source: &std::path::Path)
    -> Result<String>;
```

`reverse_dir` returns records sorted in reverse chronological order, paired
with the JSONL file each record came from.

## `describe`

```rust
pub struct Describe {
    pub description: String,
    pub version: semver::Version,
    pub stability: Stability,
    pub min_dispatcher_version: semver::Version,
    pub platforms: Vec<Platform>,
    pub project_requirements: Vec<String>,
    pub capabilities: Vec<crate::capability::CapabilityDescriptor>,
    pub command_aliases: Vec<crate::command_alias::CommandAlias>,
}

pub enum Stability { Stable, Experimental, Deprecated }
pub enum Platform { Linux, Macos, Windows }

impl Describe {
    pub fn emit_stdout(&self) -> Result<()>;
    pub fn handle_arg0_describe(args: impl IntoIterator<Item = std::ffi::OsString>)
        -> Option<ExitCode>;
}
```

## `command_alias`

```rust
pub struct CommandAlias {
    pub name: String,
    pub description: String,
    pub match_first_arg: Option<String>,
    pub target: CommandAliasTarget,
}

pub enum CommandAliasTarget {
    Set { capability: crate::capability::CapabilityId },
    Go { capability: crate::capability::CapabilityId },
    Plugin { args: Vec<String> },
}

impl CommandAlias {
    pub fn matches_args(&self, user_args: &[std::ffi::OsString]) -> bool;
    pub fn specificity(&self) -> u8;
}
```

`CommandAlias` carries a provider-declared `ready-set <name>` command. See the
[manifest](manifest.md#command-aliases) and [`__describe`](describe.md)
contracts for the wire shape; `matches_args` and `specificity` support the
dispatcher's most-specific-wins selection when several aliases share a `name`.

## `manifest`

```rust
pub struct Manifest {
    pub description: String,
    pub version: semver::Version,
    pub stability: crate::describe::Stability,
    pub min_dispatcher_version: semver::Version,
    pub platforms: Vec<crate::describe::Platform>,
    pub project_requirements: Vec<String>,
    pub capabilities: Vec<crate::capability::CapabilityDescriptor>,
    pub command_aliases: Vec<crate::command_alias::CommandAlias>,
}

impl Manifest {
    pub fn load(path: &std::path::Path) -> Result<Self>;
    pub fn sibling_of(binary: &std::path::Path) -> std::path::PathBuf;
}
```

## `sandbox`

```rust
pub trait Sandbox {
    fn declare(&mut self, capabilities: &[Capability]);
    fn enter(&self) -> Result<()>;
}

pub enum Capability {
    ReadProject,
    WriteProject,
    ReadHome,
    WriteHome,
    Network,
    Subprocess,
}

pub fn for_current_platform() -> Box<dyn Sandbox>;
```

The trait surface is `stable`. Concrete per-platform implementations are
`experimental`: in v0.1.0 they are no-op stubs that record declared
capabilities only.

## `config`

```rust
pub struct Config {
    pub path: std::path::PathBuf,
    pub ready_set: ProjectMeta,
    pub capabilities: std::collections::BTreeMap<String, CapabilityConfig>,
    pub plugins: std::collections::BTreeMap<String, PluginSection>,
    pub unknown_keys: Vec<String>,
}

pub struct ProjectMeta {
    pub schema_version: u32,
    pub profile: String,
}

pub struct CapabilityConfig {
    pub relevance: Option<crate::capability::CapabilityRelevance>,
    pub provider: Option<crate::capability::ProviderId>,
    pub unknown_keys: Vec<String>,
}

pub struct PluginSection { /* fields private */ }

impl PluginSection {
    pub fn deserialize<T>(&self) -> Result<T>
    where
        T: serde::de::DeserializeOwned;
    pub fn get_str(&self, key: &str) -> Option<&str>;
    pub fn get_bool(&self, key: &str) -> Option<bool>;
    pub fn get_integer(&self, key: &str) -> Option<i64>;
    pub fn get_string_array(&self, key: &str) -> Option<Vec<String>>;
}

pub fn load_config(start: &std::path::Path) -> Result<Option<Config>>;
pub fn parse_at(path: &std::path::Path) -> Result<Config>;
```

## `fs`

```rust
pub fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> Result<()>;
pub fn sha256_file(path: &std::path::Path) -> Result<String>;

#[cfg(unix)]
pub fn restrict_to_user(path: &std::path::Path) -> Result<()>;
```

## `dispatch`

```rust
pub struct DispatchBuilder { /* fields private */ }

impl DispatchBuilder {
    pub fn new(subcommand: impl Into<String>) -> Self;
    pub fn arg(self, arg: impl Into<std::ffi::OsString>) -> Self;
    pub fn args<I, S>(self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<std::ffi::OsString>;
    pub fn capture(self, yes: bool) -> Self;
    pub fn run(self, ctx: &Context) -> Result<DispatchOutcome>;
}

pub enum DispatchOutcome {
    Streamed { exit_code: i32 },
    Captured { stdout: Vec<u8>, exit_code: i32 },
}
```

The builder execs `ready-set <sub> ...` (going through the dispatcher, not
the plugin binary directly) so PATH semantics and built-in resolution stay
consistent. The env contract is forwarded automatically.

## `logging`

```rust
pub fn install(ctx: &Context);
```

Idempotent. Configures `tracing` + `tracing-subscriber` honoring
`Context::log_level()` and `Context::color()`.

## `error`

```rust
pub type Result<T> = std::result::Result<T, Error>;

#[non_exhaustive]
pub enum Error {
    Io(std::io::Error),
    TomlParse(String),
    JsonParse(String),
    MissingDependency { name: String, hint: Option<String> },
    ContractViolation(String),
    Other(String),
}
```

`Error` is `#[non_exhaustive]`; adding a variant in a future minor release
is non-breaking for downstream code that does not exhaustively match.
