# ready-set

The core CLI and lifecycle dispatcher for the `ready-set` ecosystem.

This crate ships the `ready-set` binary. It owns the dispatcher, the
lifecycle built-ins (`ready`, `set`, `go`), the meta commands (`help`,
`list`, `version`), the capability registry, plugin discovery, and the
dispatcher↔plugin environment contract.

It does **not** own any domain knowledge. Every Rust-, language-, or
domain-specific decision lives in a provider plugin (e.g. `ready-set-rust`).
The dispatcher's job is to route, not to act.

For the product overview, lifecycle grammar, and full architecture, see the
workspace root [`README.md`](../README.md).

## Install

```text
cargo install ready-set
```

This is enough to run the dispatcher, but the matrix only becomes useful
when at least one provider is on `PATH`. To install the first-party Rust
provider:

```text
cargo install ready-set-rust
```

Any binary on `PATH` named `ready-set-<name>` is automatically discovered.

## What the binary does

```text
ready-set                       # bare → ready (whole-product matrix)
ready-set ready [capability]    # read-only diagnosis
ready-set set   [capability]    # create / reconcile
ready-set go    [capability]    # execute workflow
ready-set <subcommand> [...]    # PATH-resolved → exec ready-set-<subcommand>
ready-set --list                # built-ins + discovered plugins
ready-set --help
ready-set --version
```

For each lifecycle verb, the dispatcher resolves the capability's provider,
exports the env contract, and execs:

```text
ready-set-<provider> __ready <capability>
ready-set-<provider> __set   <capability> [args...]
ready-set-<provider> __go    <capability> [args...]
```

Unsupported verbs are rejected by core before the provider is spawned.

## Module map

```text
ready-set/src/
├── main.rs                # binary entry; defers to lib::run
├── lib.rs                 # routing entry point used by main.rs and tests
├── cli.rs                 # meta-flag parsing (--json/--quiet/--verbose/--color)
├── builtins/
│   ├── mod.rs             # built-in route table
│   ├── ready.rs           # diagnose; renders the readiness matrix
│   ├── set.rs             # reconcile; dispatches __set to providers
│   ├── go.rs              # execute; dispatches __go to providers
│   ├── list.rs            # built-ins + discovered plugins
│   ├── help.rs help.txt   # --help text
│   └── version.rs
├── capabilities.rs        # registry merge of provider descriptors + config;
│                          #   matrix renderers (human + JSON)
├── lifecycle.rs           # __ready / __set / __go invocation; capture vs stream
├── discovery.rs           # PATH walk for ready-set-<name> binaries
├── metadata.rs            # cache → sidecar manifest → __describe waterfall
├── cache.rs               # ~/.cache/ready-set/plugins.json
├── exec.rs                # plugin exec (Unix execvp) / spawn (Windows)
├── env.rs                 # READY_SET_* env contract export
└── project.rs             # walks upward for .git / Cargo.toml / .ready-set.toml
```

`tests/` contains end-to-end coverage:

- `dispatcher_e2e.rs` — meta commands, plugin discovery, error paths.
- `lifecycle_e2e.rs` — `ready`/`set` against fake providers.
- `core_go_e2e.rs` — `go` aggregation across multiple capabilities.

## Public library surface

`ready-set` is primarily a binary crate; the library is exposed for tests
and embedders. The single entry point is:

```rust
use std::ffi::OsString;
use ready_set::run;

fn main() -> std::process::ExitCode {
    run(std::env::args_os()).into()
}
```

`run` parses argv, builds the env contract, routes to the right built-in or
plugin, and returns an
[`ready_set_sdk::ExitCode`](https://docs.rs/ready-set-sdk).

The submodules (`builtins`, `capabilities`, `cli`, `discovery`, `lifecycle`,
`metadata`, `cache`, `exec`, `env`, `project`) are public so integration
tests can assemble pieces in isolation, but they are not a stable embedder
API. The stable surface is the CLI and the contracts under
[`docs/contracts/`](../docs/contracts/).

## Plugin discovery and registry

On every invocation the dispatcher:

1. Parses meta flags and a subcommand name.
2. If the name matches a built-in, runs the built-in handler. With no name,
   runs `ready`.
3. If the name matches a capability id known to the registry, dispatches
   the relevant lifecycle protocol call to that capability's provider.
4. Otherwise, looks for `ready-set-<name>` on `PATH` and execs it.
5. If no such binary exists, exits with `ExitCode::UnknownSubcommand` and a
   hint to search crates.io.

For `--list` and the capability registry, the dispatcher walks every `PATH`
entry looking for `ready-set-*` binaries and asks each one for its metadata
via the cache → sidecar → `__describe` waterfall. Results are cached in
`~/.cache/ready-set/plugins.json` keyed by `(canonical_path, size_bytes,
head4k_sha256)` with a 24h TTL safety net.

## Environment contract

Before invoking a plugin, the dispatcher exports:

```text
READY_SET_DISPATCHER_VERSION
READY_SET_PROJECT_ROOT
READY_SET_CONFIG_PATH
READY_SET_OUTPUT
READY_SET_LOG
READY_SET_COLOR
```

The dispatcher strips unknown incoming `READY_SET_*` variables before
invoking providers. See
[`docs/contracts/env-vars.md`](../docs/contracts/env-vars.md).

## Cross-platform notes

- **Unix:** plugins are exec'd (`execvp`-style) so the plugin inherits the
  dispatcher's PID — cleaner signal handling, matches cargo.
- **Windows:** there is no `execvp`; the dispatcher spawns the plugin as a
  child via `Command::status()` and propagates its exit code. PATH discovery
  honors `PATHEXT`.
- The cache file path uses platform-conventional locations via the
  `directories` crate.

## What this crate must not do

- Hardcode capability behavior. If you find yourself writing
  `if capability == "formatting"` in this crate, you are in the wrong
  crate — extend or write a provider plugin instead.
- Let `go` create setup files. `go` is workflow execution. Setup is `set`.
- Mutate the project on `ready` or `go`. `set` is the only mutating verb.
- Link against plugins or load them dynamically. The contract is the CLI
  surface only.

## See also

- Workspace [`README.md`](../README.md) — product, lifecycle, principles,
  roadmap.
- [`AGENTS.md`](../AGENTS.md) — working guidance for coding agents.
- [`docs/contracts/`](../docs/contracts/) — versioned protocol specs.
- [`ready-set-sdk`](../ready-set-sdk/) — types and helpers used by both
  the dispatcher and provider plugins.
- [`ready-set-rust`](../ready-set-rust/) — the first-party Rust capability
  provider.

## License

Licensed under either of MIT or Apache-2.0.
