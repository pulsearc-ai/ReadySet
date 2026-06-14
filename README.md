# ready-set

**ReadySet — by [PulseArc](https://github.com/pulsearc-ai).**

`ready-set` is a product-readiness CLI: a control surface that shows what a
project has, what is missing, what is blocked, and what concrete action should
happen next.

The brand is *ready, set, go* — preparation before launch — and those three
words are the product's verbs:

```text
ready  ->  read-only diagnosis: can this product or capability be used?
set    ->  create or reconcile what is missing
go     ->  execute the capability's main workflow
```

A project is modeled as a set of *capabilities* (`workspace`, `toolchain`,
`formatting`, `linting`, `tests`, `ci`, `release`, `deploy`, `docs`,
`security`, `observability`, …). Each capability has a state in the readiness
matrix and a provider plugin that knows how to diagnose, configure, or run it.
The core dispatcher owns the grammar, the matrix, and lifecycle dispatch;
plugins own domain knowledge.

## Status

This repo is pre-v0.1.0. The capability lifecycle is implemented and binding.

| Crate | Purpose | Status |
|-------|---------|--------|
| `ready-set` | Core CLI and lifecycle dispatcher | Implemented |
| `ready-set-sdk` | Rust types and helpers for the public plugin contracts | Implemented |
| `ready-set-rust` | First-party provider plugin for Rust workspace capabilities | Implemented |

Implemented lifecycle commands:

```text
ready-set              # same as ready-set ready
ready-set ready        # show the readiness matrix
ready-set ready <id>   # diagnose one capability
ready-set set          # reconcile required set-capable capabilities
ready-set set <id>     # reconcile one capability
ready-set go           # run required go-capable workflows
ready-set go <id>      # run one capability workflow
```

`ready-set undo` is planned but is not yet routed as an available built-in.
Provider `set` mutations already write change logs and backups so the future
`undo` command has a stable data source.

## Why ready-set

Most repositories accumulate product readiness informally: a test command in a
README, a CI workflow copied from another project, release steps in a shell
script, deployment knowledge in one maintainer's head, security checks added
after something breaks.

`ready-set` makes that state explicit. It answers:

- What capabilities does this product have?
- Which capabilities are missing, stale, incomplete, or blocked?
- Which capabilities are optional or explicitly not needed?
- What is the next concrete action for each capability?
- Which workflows can run now?

### vs. a task runner

A task runner starts from commands (`test`, `build`, `deploy`). `ready-set`
starts from product capabilities (`tests`, `release`, `deployment`,
`observability`, `security`). Commands answer "what can I run?" — capabilities
answer "what does this product have, what is it missing, and what can I
trust?" Execution is a result of product understanding rather than the whole
product.

### vs. workspace readiness alone

Workspace readiness asks "is this repo configured correctly?" Capability
readiness asks "does this product have the pieces it needs?" The second
question is larger. A repo can have a pinned toolchain, lints, and formatting
while still lacking CI, release automation, deployment, docs, or operational
checks. Workspace readiness becomes one capability among many.

## Quick start

Build the workspace and put local binaries on `PATH` so the core can discover
the `ready-set-rust` provider:

```text
cargo build --workspace
PATH="$PWD/target/debug:$PATH" target/debug/ready-set ready
```

Useful local commands:

```text
PATH="$PWD/target/debug:$PATH" target/debug/ready-set --list
PATH="$PWD/target/debug:$PATH" target/debug/ready-set ready
PATH="$PWD/target/debug:$PATH" target/debug/ready-set set formatting
PATH="$PWD/target/debug:$PATH" target/debug/ready-set go formatting
PATH="$PWD/target/debug:$PATH" target/debug/ready-set --json ready
```

The Rust provider runs inside Cargo projects. `set` may write files and change
logs. `ready` is read-only. `go` runs workflows and never writes setup files.

## The readiness matrix

The central experience is a readiness matrix. Each row is a capability with a
state, a next action, and a one-line summary.

Example:

```text
capability   state      next action                summary
formatting   missing    ready-set set formatting   rustfmt.toml is missing
linting      ready      -                          linting configuration is ready
toolchain    ready      -                          rust-toolchain.toml present
workspace    ready      -                          workspace configuration is ready
```

Supported readiness states:

```text
ready         present and usable
missing       no implementation exists
incomplete    present, but not fully configured
blocked       cannot be evaluated until a dependency is resolved
stale         exists, but no longer matches the declared product state
optional      available, but not required for this product
not-needed    explicitly irrelevant for this product
```

Supported relevance values:

```text
required      affects readiness; missing/incomplete/blocked/stale fails the
              whole-product check
optional      surfaced in the matrix, but does not fail the whole-product check
not-needed    short-circuits without provider execution
```

`ready-set ready` exits non-zero when any *required* selected capability is
`missing`, `incomplete`, `blocked`, or `stale`.

## Command reference

### `ready-set`

With no subcommand, runs the same path as `ready-set ready` and prints the
readiness matrix.

### `ready-set ready [capability]`

Read-only diagnosis. Core builds the capability registry, dispatches `__ready`
to providers, and renders a matrix.

In JSON mode it emits an array of `CapabilityReport` objects:

```text
ready-set --json ready
ready-set --json ready linting
```

`ready` never writes files or change logs.

### `ready-set set [capability]`

Setup and reconciliation. Without a capability argument, core selects all
required capabilities that support `set`. With an argument, it reconciles
exactly that capability.

Provider mutations record JSONL change logs under `.ready-set/changes/` and
backups under `.ready-set/backups/`.

```text
ready-set set
ready-set set linting
ready-set set --dry-run linting
ready-set set --force formatting
```

`set --dry-run` writes nothing.

### `ready-set go [capability]`

Workflow execution. Without a capability argument, core selects all required
capabilities that support `go`. With an argument, it runs exactly that
capability workflow. Multiple selected workflows continue after failures and
the command exits non-zero if any failed.

```text
ready-set go formatting
ready-set go linting
ready-set --json go formatting
```

For the Rust provider:

- `formatting` runs `cargo fmt --check`.
- `linting` runs `cargo clippy --workspace --all-targets`.

`go` never bootstraps missing files. Selecting a capability that does not
support `go` (for example `workspace` or `toolchain`) is a user error before
any provider is spawned.

### Meta commands

```text
ready-set --help
ready-set --version
ready-set --list
ready-set --list --all
```

`--list` shows routed built-ins and discovered `ready-set-*` plugins. It hides
plugins whose manifest excludes the current platform unless `--all` is passed.

Global flags:

```text
--quiet         errors only (READY_SET_LOG=quiet)
--verbose       debug logging (READY_SET_LOG=verbose)
--json          machine-readable output (READY_SET_OUTPUT=json)
--color <when>  auto (default) | always | never
```

Global flags are forwarded to providers via the env contract.

## Design principles

These principles are binding for any change to the core or the SDK contract.

1. **Small core + provider plugins.** The `ready-set` binary hosts the
   dispatcher, lifecycle built-ins (`ready`, `set`, `go`), and meta-commands
   (`help`, `list`, `version`). Domain capabilities live in
   `ready-set-<name>` provider plugins discovered on PATH.
2. **Capabilities are first-class.** The unit of work is a capability, not a
   subcommand. Plugins contribute capabilities; the core aggregates them
   into the readiness matrix and dispatches lifecycle verbs.
3. **Each plugin stands alone.** A user installs only the providers they
   need. A broken plugin cannot crash the dispatcher or other plugins.
4. **Default to language-agnostic; specialize when needed.** The first-party
   `ready-set-rust` provider activates inside a cargo context. Other
   providers can be language-neutral.
5. **Cross-platform from PR #1.** Linux, macOS, Windows from one codebase.
   No `sh -c`, no hardcoded path separators, no shell pipelines.
6. **Opinionated defaults, escape hatches available.** The bare command
   shows the matrix. `set` reconciles required capabilities with sensible
   defaults. Advanced behavior is unlocked with explicit flags or
   `.ready-set.toml`.
7. **Composability over completeness.** Each capability is useful in
   isolation, scriptable, and produces machine-readable output (`--json`).
8. **Reversibility.** `set` mutations are recorded to a per-project change
   log so the planned `ready-set undo` can reverse them. `ready` and `go` do
   not write files.
9. **No telemetry.** If ever added, opt-in, documented, disabled in CI.
10. **Stable contracts over fast iteration.** The dispatcher↔plugin surface
    is a long-term API. Designed carefully before v0.1.0; post-v0.1.0
    changes are semver-breaking for the core.

### Built-in vs. plugin

Built-ins are reserved for:

- The lifecycle grammar (`ready`, `set`, `go`).
- Dispatcher meta-commands (`help`, `list`, `version`, future `completions`).
- Bootstrap-of-the-bootstrapper.
- Ecosystem contracts that must work across plugins. `undo` qualifies because
  it reverses change records regardless of which plugin produced them.

Everything else is a plugin. When in doubt, plugin.

### Product principles for capability authors

- **Make product state visible.** One command should reveal what exists, what
  is missing, what is blocked.
- **Prefer concrete next actions.** Every non-ready state should point to a
  next action when possible.
- **Keep capabilities opinionated but escapable.** The common path works with
  zero flags. Advanced users can mark capabilities optional, ignored, or
  externally managed via `.ready-set.toml`.
- **Don't pretend every product is the same.** A library, CLI, web service,
  desktop app, and internal tool should not be judged against the same fixed
  checklist.
- **Keep `go` meaningful.** `go` executes product capabilities, not arbitrary
  aliases. If everything can be a `go` target, the command loses meaning.
- **Preserve trust.** `ready` is read-only. `set` records every mutation.
  `go` is clear about what it will execute.

## Architecture

### Lifecycle dispatch

The dispatcher routes one of two ways: meta-commands (`--help`, `--list`,
`--version`) and lifecycle verbs that resolve through the capability
registry.

```text
ready-set                       # bare → ready (whole-product matrix)
ready-set ready [capability]    # read-only diagnosis
ready-set set   [capability]    # create / reconcile
ready-set go    [capability]    # execute workflow
ready-set <subcommand> [...]    # PATH-resolved → exec ready-set-<subcommand>
ready-set --list                # built-ins + discovered plugins
```

For a verb that names a capability, the core resolves the capability's
provider, exports the env contract, and execs:

```text
ready-set-<provider> __ready <capability>
ready-set-<provider> __set   <capability> [args...]
ready-set-<provider> __go    <capability> [args...]
```

`__ready` captures one JSON `CapabilityReport`. `__set` streams normal output
(or captures JSON when `--json` is set globally) and records mutations
through the change log. `__go` runs the capability's main workflow.
Unsupported verbs are rejected by core before the provider is spawned.

For a name that is not a built-in and not a known capability, the dispatcher
falls through to PATH plugin discovery: `ready-set-<name>` as a freestanding
subcommand, exactly like cargo's plugin model. If no such binary exists, the
dispatcher exits with a clear error and a hint to search crates.io for
`ready-set-*`.

The dispatcher does not link against plugins, does not load them dynamically,
and does not trust them with anything beyond the args the user typed plus the
documented env contract.

### Plugin discovery

The dispatcher does not eagerly enumerate plugins. On each invocation it:

1. Parses meta flags and a subcommand name.
2. If the name matches a built-in, runs the built-in handler. With no name,
   runs `ready`.
3. If the name matches a capability id known to the registry, dispatches the
   relevant lifecycle protocol call to that capability's provider.
4. Otherwise, looks for a binary named `ready-set-<name>` on PATH and execs
   it with the remaining args (cargo plugin model).

For `--list` and the capability registry, the dispatcher walks every `PATH`
entry looking for `ready-set-*` binaries and asks each one for its metadata
via the cache → sidecar → `__describe` waterfall. Results are cached in
`~/.cache/ready-set/plugins.json` keyed by a hash of `(binary path, binary
size, first 4 KB of contents)` with a 24h TTL safety net. The cache file
carries a `schema_version` field.

### Workspace layout

```text
ready-set/                          # repo + cargo workspace
├── Cargo.toml                      # [workspace] members
├── README.md                       # this file
├── AGENTS.md                       # working guidance for coding agents
├── docs/contracts/                 # public plugin and wire contracts
├── ready-set/                      # core CLI (bin: ready-set)
│   └── src/
│       ├── main.rs                 # dispatcher entry
│       ├── lib.rs                  # routing
│       ├── cli.rs                  # meta-flag parsing
│       ├── capabilities.rs         # registry + matrix
│       ├── lifecycle.rs            # __ready/__set/__go invocation
│       ├── discovery.rs            # PATH walk, plugin metadata
│       ├── exec.rs                 # plugin exec + env contract
│       ├── metadata.rs             # manifest / __describe resolution
│       ├── cache.rs                # plugin metadata cache
│       └── builtins/               # ready, set, go, help, list, version
├── ready-set-sdk/                  # library crate for plugin authors
│   └── src/
│       ├── capability.rs           # descriptor / report / run-report types
│       ├── lifecycle.rs            # parsing of __ready/__set/__go requests
│       ├── manifest.rs describe.rs config.rs change_log.rs ...
└── ready-set-rust/                 # first-party provider plugin
    └── src/
        ├── main.rs                 # subcommand entry incl. __ready/__set/__go
        ├── readiness.rs            # per-capability state evaluation
        ├── runner.rs               # __set implementation
        ├── workflow.rs             # __go implementation
        └── workspace.rs templates.rs ...
```

Third-party plugins live in their own repos.

## The Rust provider

`ready-set-rust` is the first-party provider for Rust workspace foundations.
It uses provider id `rust`.

| Capability | Verbs | Responsibility |
|------------|-------|----------------|
| `workspace` | `ready`, `set` | Cargo workspace shape, `.gitignore`, `.ready-set.toml` |
| `toolchain` | `ready`, `set` | `rust-toolchain.toml` |
| `formatting` | `ready`, `set`, `go` | `rustfmt.toml`, `cargo fmt --check` |
| `linting` | `ready`, `set`, `go` | `clippy.toml`, workspace lints, `cargo clippy --workspace --all-targets` |

Direct provider protocol examples:

```text
ready-set-rust __describe
ready-set-rust __ready linting
ready-set-rust __set linting
ready-set-rust __go linting
```

Users normally go through the core:

```text
ready-set ready linting
ready-set set linting
ready-set go linting
```

## Authoring a provider plugin

A plugin is a standalone binary named `ready-set-<name>` (`.exe` on Windows).
It can be written in any language; first-party plugins are Rust crates
depending on `ready-set-sdk`. To participate in the lifecycle, a plugin
declares its capabilities in metadata and answers the lifecycle protocol
calls.

### Minimum Rust shape

```rust
// crate: ready-set-rust, bin: ready-set-rust
use ready_set_sdk::prelude::*;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let ctx = Context::from_env();
    match args.get(1).and_then(|s| s.to_str()) {
        Some("__describe") => describe::print(&MANIFEST),
        Some("__ready") => lifecycle::dispatch_ready(&args[2..], &ctx, ready_impl),
        Some("__set")   => lifecycle::dispatch_set(&args[2..], &ctx, set_impl),
        Some("__go")    => lifecycle::dispatch_go(&args[2..], &ctx, go_impl),
        _ => run_user_subcommand(&args, &ctx),
    }
}
```

Plugins that do not provide capabilities skip the lifecycle handlers and
declare `capabilities: []` in their manifest.

### What the SDK provides

- **`Context`** — per-invocation state from the env contract.
- **Capability types** — `CapabilityDescriptor`, `CapabilityId`,
  `CapabilityVerb`, `CapabilityState`, `CapabilityRelevance`,
  `CapabilityReport`, `CapabilityRunReport`, `CapabilityAction`,
  `NextAction`, `ProviderId`, `RunStatus`. Serde round-tripping with the
  contract schemas.
- **Lifecycle helpers** — request parsing for `__ready`/`__set`/`__go`,
  response emission for reports and run reports.
- **Manifest + describe** — typed manifest model with required `capabilities`
  field; `__describe` printer.
- **Config loading** — reads `.ready-set.toml` v2: profile,
  `[capabilities.<id>]` overrides, plus per-plugin sections. Rejects v1.
- **Output formatting** — `Output::human()` / `Output::json()`.
- **Logging** — `tracing` setup with `--verbose`/`--quiet` honored.
- **Filesystem helpers** — atomic writes, change records for reversibility.
- **Exit codes** — `ExitCode` enum mapped to the contract codes.
- **Cross-plugin composition** — `ready_set_sdk::dispatch("scan", &["--json",
  "..."])` execs through the dispatcher and forwards the env contract.

The SDK is versioned independently. Breaking changes follow semver; plugins
pin a major version. The dispatcher does not check SDK version compatibility
— there is no in-process API surface between core and plugins, only the CLI
contract.

### Capability metadata

Providers declare capabilities through a manifest sidecar or `__describe`
payload. A descriptor has this shape:

```json
{
  "id": "linting",
  "title": "Linting",
  "provider": "rust",
  "verbs": ["ready", "set", "go"],
  "default_relevance": "required"
}
```

Rules:

- `id` is the stable capability id. Lowercase kebab-case:
  `^[a-z][a-z0-9-]*$`. `ready`, `set`, `go` are reserved.
- `title` is the human display name.
- `provider` is the provider id, usually the plugin name without
  `ready-set-`.
- `verbs` declares exactly which lifecycle verbs are supported.
- `default_relevance` is `required`, `optional`, or `not-needed`.
- Plugin metadata must include `capabilities`, using `[]` when the plugin has
  no lifecycle capabilities.

The full contract lives in
[`docs/contracts/capabilities.md`](docs/contracts/capabilities.md).

### Plugin metadata delivery

Two delivery mechanisms exist:

- **`__describe` subcommand (cargo-install baseline)** — invoked as
  `ready-set-<name> __describe`. The plugin prints a single line of JSON to
  stdout and exits 0 within 100 ms, with no other side effects.
- **Manifest sidecar (fast path)** — a `ready-set-<name>.toml` file installed
  alongside the binary on PATH. Read without spawning the plugin. When both
  exist, the sidecar wins if its schema version is supported.

Schema (same shape for both):

```json
{
  "description": "Rust workspace foundation provider.",
  "version": "0.1.0",
  "stability": "stable",
  "min_dispatcher_version": "0.1.0",
  "platforms": ["linux", "macos", "windows"],
  "project_requirements": ["cargo-workspace"],
  "capabilities": [
    {
      "id": "workspace",
      "title": "Workspace",
      "provider": "rust",
      "verbs": ["ready", "set"],
      "default_relevance": "required"
    },
    {
      "id": "formatting",
      "title": "Formatting",
      "provider": "rust",
      "verbs": ["ready", "set", "go"],
      "default_relevance": "required"
    }
  ]
}
```

See:

- [`docs/contracts/manifest.md`](docs/contracts/manifest.md)
- [`docs/contracts/describe.md`](docs/contracts/describe.md)
- [`docs/contracts/cache.md`](docs/contracts/cache.md)

### Naming subcommands and capabilities

- **Verbs preferred for plugin names.** `scan`, `template`, `harden` — not
  `scanner`, `templater`.
- **Capability ids are nouns.** `formatting`, `linting`, `tests`, `release`.
  Lowercase kebab-case.
- **`ready`, `set`, `go` are reserved** for the lifecycle built-ins.
- **Plugin crate names mirror the binary.** `ready-set scan` is the crate
  `ready-set-scan` producing the binary `ready-set-scan`.
- **Avoid clashes with cargo built-ins** in case a future cargo-subcommand
  affordance lands.
- **Reserve the `ready-set-*` namespace responsibly.** Crates.io is global;
  squatting is bad citizenship. Publish only when the plugin actually
  exists.

### Subcommand contract

Every subcommand (built-in or plugin):

- Accepts `--json` for machine-readable output; defaults to human output.
- Accepts `--quiet` (errors only) and `--verbose` (debug logging).
- Returns exit codes from
  [`docs/contracts/exit-codes.md`](docs/contracts/exit-codes.md).
- Is idempotent where possible. If not, says so in `--help` and produces a
  record of changes.
- Documents required filesystem/network access in `--help`.

Enforced by convention and the SDK, not by the dispatcher — the dispatcher
cannot inspect a plugin's internals.

## Dispatcher environment contract

Before invoking a plugin, the dispatcher exports:

```text
READY_SET_DISPATCHER_VERSION   semver of the core that invoked the plugin
READY_SET_PROJECT_ROOT         absolute path to resolved project root, or unset
READY_SET_CONFIG_PATH          absolute path to resolved .ready-set.toml, or unset
READY_SET_OUTPUT               "human" | "json"
READY_SET_LOG                  "quiet" | "normal" | "verbose"
READY_SET_COLOR                "auto" | "always" | "never"
```

Lifecycle invocations (`__ready`, `__set`, `__go`) use the same env contract.
Plugins must tolerate unset or unrecognized values for forward compatibility.
The dispatcher strips unknown incoming `READY_SET_*` variables before
invoking providers. The `READY_SET_*` namespace is reserved for future
contract additions.

See [`docs/contracts/env-vars.md`](docs/contracts/env-vars.md).

## `.ready-set.toml`

Project-local configuration lives at `.ready-set.toml`.

Minimal file:

```toml
[ready-set]
schema_version = 2
profile = "rust-workspace"
```

Capability overrides:

```toml
[capabilities.linting]
relevance = "required"
provider = "rust"

[capabilities.deploy]
relevance = "not-needed"
```

Behavior:

- `schema_version` must be `2`. v1 is rejected.
- `relevance` overrides the descriptor's default relevance.
- `provider` selects a provider when more than one descriptor can satisfy an
  id.
- Unknown capability ids are preserved by config parsing but do not appear in
  the registry unless a provider descriptor exists.

See [`docs/contracts/ready-set-toml.md`](docs/contracts/ready-set-toml.md).

## Output and exit codes

Human output is the default. `--json` selects machine-readable output:

- `ready` emits `CapabilityReport` arrays.
- `set` and `go` emit `CapabilityRunReport` arrays from core lifecycle
  commands.
- Provider `__ready` emits one `CapabilityReport`.
- Provider `__set` and `__go` emit one `CapabilityRunReport` in JSON mode.

Exit codes are shared across built-ins and plugins. See
[`docs/contracts/exit-codes.md`](docs/contracts/exit-codes.md).

## Reversibility

`set` is the mutating lifecycle verb. Mutating providers record each write as
a JSONL change record under:

```text
.ready-set/changes/<provider>-<timestamp>-<rand>.jsonl
```

Each line:

```json
{"op": "create" | "modify" | "delete", "path": "...", "before_sha256": "...", "after_sha256": "..."}
```

When a file is modified or deleted, the provider saves pre-change content
under:

```text
.ready-set/backups/<sha256>
```

The change log lives inside the project so it travels with the workspace, is
visible in `git status`, and can be committed if the user wants the audit
trail in version control.

The planned `ready-set undo` built-in will consume these records in reverse
chronological order regardless of which plugin produced them. It refuses to
reverse a record if `after_sha256` no longer matches the file's current
contents (the user has edited it post-mutation), unless invoked with
`--force`. The change-log format is already specified and implemented in the
SDK.

See [`docs/contracts/change-log.md`](docs/contracts/change-log.md).

## Cross-platform support

`ready-set` runs on Linux, macOS, and Windows. CI verifies all three on every
push and merge request; release artifacts are published for all three.

Conventions:

- **Paths.** Always `std::path::Path`/`PathBuf`. Never string-concat with `/`
  or `\`. Display paths to users with `Path::display()`.
- **Line endings.** Read text files with `\n` normalization; write `\n` by
  default. When round-tripping a file (e.g., editing `Cargo.toml` in place),
  preserve its original line endings to avoid spurious diffs on Windows.
- **File permissions.** Unix permission bits apply on Unix only. On Windows,
  restrict via ACLs or accept the OS default. Gate calls behind
  `#[cfg(unix)]`.
- **External commands.** Invoke via `std::process::Command`, never via
  `sh -c` or `cmd /c`. Look up binary names without extension; the OS
  resolves `.exe` on Windows. Use the `which` crate to verify availability
  before invoking.
- **Plugin discovery.** Walk `PATH` looking for `ready-set-<name>` on Unix
  and `ready-set-<name>.exe` on Windows. Honor `PATHEXT` on Windows. The
  cache file path uses platform-conventional locations via the `directories`
  crate.
- **Plugin exec vs. spawn.** On Unix, prefer `execvp`-style replacement so
  the plugin inherits the dispatcher's PID — cleaner signal handling, matches
  cargo. On Windows, no `execvp`; spawn the plugin as a child via
  `Command::status()` and propagate its exit code.
- **Home and config dirs.** Use the `directories` crate; never read
  `$HOME`/`%USERPROFILE%` directly.
- **Symlinks.** Windows allows symlink creation when Developer Mode is
  enabled or the process is elevated. Use a fallback chain: (1) symlink, (2)
  directory junction or hard link, (3) copy with a clear warning.
- **Case sensitivity.** Treat filesystems as case-sensitive in code.
- **Terminal output.** Detect TTY and color support; never assume ANSI escape
  codes work. `--json` is the always-safe machine output.

Capabilities that genuinely cannot work on a platform declare it explicitly:
the descriptor's `verbs` reflects the supported set per platform and `--help`
says "Linux only" with a clear error elsewhere. The dispatcher never lies
about availability.

## Stability and versioning

- **Independent versioning.** Each crate (`ready-set`, `ready-set-sdk`,
  `ready-set-rust`, third-party plugins) versions on its own schedule. There
  is no "suite version."
- **Compatibility is at the protocol level**, not the API level. Core never
  links to plugins; the contract is the CLI args, exit codes, output schema,
  env vars, manifest sidecar / `__describe`, `__ready`/`__set`/`__go` shapes,
  capability descriptor / report / run-report schemas, change log JSONL, and
  `.ready-set.toml` v2. Breaking any of those is a breaking change for the
  core.
- **SDK semver matters for plugin authors.** Plugins pin a major version of
  `ready-set-sdk`.
- **Stability tiers.** Each plugin and capability declares `stable`,
  `experimental`, or `deprecated` via metadata. Experimental items can break
  in any release; stable ones follow semver.
- **No coordinated suite releases.** Components ship when ready.

Within `ready-set 0.x.y`:

- **Adding** a new optional field to a stable contract is a *minor* change.
- **Adding** a new optional `READY_SET_*` env var is a *minor* change.
- **Removing** a field, **changing** the semantics of an existing field, or
  **renaming** anything is a *breaking* change for the dispatcher and
  requires a major-version bump.

Plugin authors should treat the stable contracts as a forward-compatible
surface: tolerate unknown fields, ignore unrecognized values, and never
hardcode field counts.

See [`docs/contracts/README.md`](docs/contracts/README.md) for the
authoritative stability tier of every contract.

## Distribution

Today:

- **Core: `cargo install ready-set`** — installs the dispatcher with the
  lifecycle built-ins.
- **Provider: `cargo install ready-set-rust`** — adds the first-party Rust
  workspace provider.
- **Plugins: `cargo install ready-set-<name>`** — any provider or plugin; the
  dispatcher picks it up automatically.
- **Pre-built binaries** — release artifacts for the core (and first-party
  providers) for common targets (Linux x86_64/aarch64, macOS x86_64/aarch64,
  Windows x86_64/aarch64) on tag push.

Future:

- **Homebrew tap** — `brew install <user>/ready-set/ready-set`.
- **Container image** — for CI without a Rust toolchain. May ship a "kitchen
  sink" image with first-party providers preinstalled.
- **Shell completions** — `ready-set completions <shell>`. Plugins can ship
  their own completions installed into a shared directory.
- **Plugin discovery / search** — `ready-set search <query>` queries
  crates.io for `ready-set-*` crates.

## Roadmap

The capability lifecycle is implemented and binding. The remaining work is
release infrastructure and the planned `undo` built-in.

### Done

1. **Capability contract** — descriptor / report / run-report shapes, JSON
   schemas, manifest + describe updates.
2. **SDK capability types** — typed Rust mirrors with serde round trips.
3. **`.ready-set.toml` schema v2** — profile + `[capabilities.<id>]` tables.
   v1 rejected.
4. **Core capability registry** — discovers descriptors, merges config
   overrides, renders human + JSON matrices.
5. **`ready` built-in and bare command** — read-only diagnosis through
   provider dispatch; bare `ready-set` shows the matrix.
6. **Provider-dispatched `set` and Rust plugin extraction** — Rust foundation
   behavior moved out of core into `ready-set-rust`.
7. **Lifecycle `go` built-in** — `go` no longer bootstraps; it dispatches to
   provider workflows. Formatting and linting are the first `go`-capable
   Rust capabilities.
8. **Plugin capability dispatch hardening** — verb validation, run-report
   parsing, env contract parity across `ready` / `set` / `go`.
9. **User-facing docs** — `--help` and `--list` text reflect the lifecycle.

### Remaining After v0.1.0

10. **`undo` built-in** — reverses `.ready-set/changes/` records regardless
    of provider.
11. **CI and release infrastructure** — pipeline + signed release binaries
    on tag push.

### Release-readiness gates

These gates define the long-term release target for the dispatcher and
first-party providers. `ready-set undo` is tracked as post-v0.1.0 work.

- `ready-set ready` correctly classifies a fresh cargo workspace and the
  same workspace after `ready-set set` on Linux, macOS, and Windows.
- `ready-set set` end-to-end in a fresh cargo workspace produces the Rust
  foundation files and writes change records under `rust`.
- `ready-set go formatting` and `ready-set go linting` invoke the provider
  workflows and report failure correctly.
- A reference plugin (no SDK) declaring one capability participates in the
  matrix and answers `__ready` / `__set` correctly, exercising the env and
  lifecycle contracts.
- `ready-set --list` discovers the reference plugin on all three platforms.
- Each contract spec under `docs/contracts/` is published with a version
  number and stability tier.
- SDK v0.1.0 documentation passes `cargo doc` with no warnings; every public
  item has a doc comment.

## Example product profiles

Profiles illustrate which capabilities a product type typically needs.
Profile detection is not yet implemented; the matrix today is config-driven.

### Rust library

```text
toolchain    formatting    linting    tests    ci    docs    release    security
```

Likely not needed: `deploy`, `database`, `observability`.

### Web service

```text
toolchain    formatting    linting    tests    ci    config    secrets
database     deploy        observability       security        release
```

### CLI

```text
toolchain    formatting    linting    tests    ci    packaging    release    docs    security
```

A `.ready-set.toml` `profile` field selects expectations; auto-detection of
profile is on the open-questions list.

## Open questions

These are deliberately not decided. Revisit when the framework hits them.

- **Capability dependencies.** Do we model `ready ci` as blocked when
  `tests` is `missing`? The current registry is flat; any dependency graph
  is a contract addition. Defer until a real cross-capability blocker
  appears.
- **Product profile detection.** Per-profile expected-capability lists
  (library / web service / CLI). Today: config-driven. Auto-detection is
  unspecified.
- **"Next safe action" ranking.** When multiple capabilities are actionable,
  which one should `ready-set` recommend first?
- **Irreversibility model for `go` actions.** What is safe to run vs. what
  requires confirmation?
- **Capability id squatting.** A third-party plugin claiming `id: "tests"`
  collides with whatever first-party plugin lands later. Today the first
  discovered provider wins unless config selects another. We may need a
  registry / soft-namespacing later.
- **Plugin sandboxing.** Should the dispatcher enforce any sandboxing on
  plugins it execs? Cargo trusts plugins fully. Defer.
- **Plugin authentication / signing.** Should published plugins be signed,
  or attested via crates.io / sigstore? Today: no.
- **Configuration vs. flags.** Where is the line between `.ready-set.toml`
  and explicit flags? Default to flags; promote to config when the same
  value is needed across many invocations.
- **Cargo-subcommand affordance.** Should `cargo ready-set <args>` work via
  a `cargo-ready-set` shim? Defer until a real user reports muscle-memory
  friction.
- **Promotion path.** When does a popular plugin earn promotion to a
  built-in? Probably never — the bar is the lifecycle grammar plus
  cross-plugin contracts.

## Developing in this workspace

Prerequisites:

- Rust toolchain matching `rust-toolchain.toml`.
- Cargo on `PATH`.

Common checks:

```text
cargo fmt --check
cargo test -p ready-set-sdk
cargo test -p ready-set
cargo test -p ready-set-rust
cargo test --workspace
cargo clippy --workspace --all-targets
```

Stale-wording scan when touching lifecycle docs or command behavior:

```text
rg -n 'Bootstrap[[:space:]]+a[[:space:]]+Rust[[:space:]]+workspace|bootstrap[ -]built[ -]in|ready-set[[:space:]]+go.*bootstraps?|original[[:space:]]+bootstrap[[:space:]]+value|pending .*ready-set[[:space:]]+go|bootstrap[[:space:]]+change[[:space:]]+logs' ready-set ready-set-rust ready-set-sdk docs README.md
```

## Repository guidance for agents

Read [`AGENTS.md`](AGENTS.md) before making changes. The short version:

- Keep core generic.
- Put domain behavior in providers.
- Do not make `go` create setup files.
- Keep `ready` read-only.
- Keep `set --dry-run` read-only.
- Update contracts, SDK types, docs, and tests together when protocol shapes
  change.

## Documentation map

This README is the primary root-level overview. Supporting docs:

- [`AGENTS.md`](AGENTS.md): working guidance for coding agents.
- [`docs/contracts/`](docs/contracts/): public plugin and wire contracts,
  with JSON schemas under [`docs/contracts/schemas/`](docs/contracts/schemas/).
- [`ready-set/README.md`](ready-set/README.md): the core dispatcher crate.
- [`ready-set-sdk/README.md`](ready-set-sdk/README.md): the plugin author SDK.
- [`ready-set-rust/README.md`](ready-set-rust/README.md): the first-party
  Rust capability provider.

For implementation decisions, prefer this README plus the contract docs.

## License

Licensed under either of:

- [MIT](LICENSE-MIT)
- [Apache-2.0](LICENSE-APACHE)

at your option.
