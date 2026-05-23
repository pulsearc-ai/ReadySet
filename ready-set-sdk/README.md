# ready-set-sdk

**ReadySet — by [PulseArc](https://github.com/pulsearc-ai).**

Shared conventions and helpers for `ready-set` plugins.

This crate is the typed Rust mirror of the contracts under
[`docs/contracts/`](https://github.com/pulsearc-ai/ready-set/tree/main/docs/contracts)
plus the library helpers that first-party plugins use to participate in
the lifecycle. It is **not required** — any binary on `PATH` named
`ready-set-<name>` can be a plugin — but using the SDK keeps plugins
consistent with the dispatcher and first-party tools, and saves writing
the same boilerplate across crates.

For the lifecycle grammar, plugin model, and ecosystem overview, see the
workspace root
[`README.md`](https://github.com/pulsearc-ai/ready-set/blob/main/README.md).

## Install

```toml
[dependencies]
ready-set-sdk = "0.1.0-alpha.1"
```

While the SDK is in the `0.1.0-alpha.*` series, pin the exact pre-release
version. Cargo does not select pre-release versions for `^0.1` ranges.
Once `0.1.0` ships, `ready-set-sdk = "0.1"` will pick up patch releases as
usual.

The SDK is versioned independently of the dispatcher. Plugins pin a major
version; the dispatcher does not check SDK version compatibility because
there is no in-process API surface between core and plugins, only the CLI
contract.

## Minimal plugin shape

```rust
use ready_set_sdk::describe::{Describe, Platform, Stability};
use ready_set_sdk::prelude::*;

fn describe() -> Describe {
    Describe {
        description: "Example plugin".into(),
        version: "0.1.0".parse().unwrap(),
        stability: Stability::Experimental,
        min_dispatcher_version: "0.1.0".parse().unwrap(),
        platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
        requires_cargo_workspace: false,
        capabilities: Vec::new(),
    }
}

fn main() -> std::process::ExitCode {
    let descr = describe();
    if let Some(code) = descr.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }
    let ctx = Context::from_env();
    let mut out = Output::for_context(&ctx, std::io::stdout());
    out.human("hello world");
    ExitCode::Ok.into()
}
```

A full runnable version lives at
[`examples/minimal_plugin.rs`](https://github.com/pulsearc-ai/ready-set/blob/main/ready-set-sdk/examples/minimal_plugin.rs):

```text
cargo run --example minimal_plugin -- __describe
READY_SET_OUTPUT=human cargo run --example minimal_plugin
```

## Adding lifecycle support

Plugins that contribute capabilities answer `__ready`, `__set`, and `__go`.
Use `parse_lifecycle_request` to discriminate them:

```rust
use ready_set_sdk::{Context, ExitCode, LifecycleRequest, parse_lifecycle_request};

fn main() -> std::process::ExitCode {
    let descr = my_describe();
    if let Some(code) = descr.handle_arg0_describe(std::env::args_os()) {
        return code.into();
    }

    let request = match parse_lifecycle_request(std::env::args_os()) {
        Ok(Some(r)) => r,
        Ok(None) => return ExitCode::Ok.into(),
        Err(err) => {
            eprintln!("plugin: {err}");
            return ExitCode::UserError.into();
        }
    };

    let ctx = Context::from_env();
    match request {
        LifecycleRequest::Ready { capability } => run_ready(&ctx, capability.as_str()).into(),
        LifecycleRequest::Set   { capability, args } => run_set(&ctx, capability.as_str(), &args).into(),
        LifecycleRequest::Go    { capability, args } => run_go(&ctx, capability.as_str(), &args).into(),
    }
}
```

The provider responds:

- `__ready` emits one JSON `CapabilityReport`.
- `__set` and `__go` emit `CapabilityRunReport` in JSON mode (or stream
  human output otherwise) and record mutations through the change log.

Unsupported verbs are rejected by the dispatcher before the plugin is
spawned, based on the `verbs` array in each `CapabilityDescriptor`.

## What lives here

| Module | Purpose |
|--------|---------|
| `context` | Per-invocation state ([`Context`]), populated from the `READY_SET_*` env contract. |
| `capability` | Capability descriptors, reports, and run reports — typed mirror of `docs/contracts/capabilities.md`. |
| `output` | Human/JSON output formatting (`Output`, `OutputMode`). |
| `exit_code` | Documented process exit codes (`ExitCode`). |
| `change_log` | Append-only JSONL change log + content-addressed backups for reversibility. |
| `describe` | `__describe` subcommand support; manifest schema reused by sidecars. |
| `manifest` | `ready-set-<name>.toml` sidecar parsing. |
| `config` | `.ready-set.toml` v2 loader (rejects v1). |
| `lifecycle` | `LifecycleRequest` parsing for `__ready` / `__set` / `__go`. |
| `dispatch` | Cross-plugin dispatch via the core (forwards env contract automatically). |
| `fs` | Atomic writes, hashing, content-addressed backup helpers. |
| `logging` | `tracing` setup honoring `--quiet` / `--verbose` / color preferences. |
| `sandbox` | Per-platform sandbox trait. v0.1.0 implementations are no-op stubs. |
| `error` | SDK-wide [`Error`] / [`Result`] (non-exhaustive). |
| `prelude` | Convenient re-exports for plugin authors. |

## Re-exports

`use ready_set_sdk::prelude::*;` brings in the most-used items:

```rust
CapabilityAction, CapabilityActionKind, CapabilityDescriptor, CapabilityId,
CapabilityRelevance, CapabilityReport, CapabilityRunReport, CapabilityState,
CapabilityVerb, NextAction, ProviderId, RunStatus,
ColorMode, Context, LogLevel,
Error, Result,
ExitCode,
LifecycleRequest, LifecycleRequestError, parse_lifecycle_request,
Output, OutputMode,
```

The crate root (`ready_set_sdk::*`) also re-exports the contract types,
`Context`, `Error`, `Result`, `ExitCode`, the lifecycle helpers, and
`Output`/`OutputMode`.

## Reversibility helpers

Mutating providers should record each filesystem write to the project's
change log so the planned `ready-set undo` can reverse it across providers.
The SDK gives you:

- `change_log::ChangeLog` — open an append-only JSONL file under
  `.ready-set/changes/<provider>-<timestamp>-<rand>.jsonl`.
- `change_log::ChangeRecord` / `ChangeOp` — typed entries with
  `before_sha256` / `after_sha256` fields.
- `change_log::backup_file` — copy pre-mutation content into
  `.ready-set/backups/<sha256>` (content-addressed).

`set --dry-run` must not write change logs or backups. The SDK helpers
expose dry-run aware variants.

See
[`docs/contracts/change-log.md`](https://github.com/pulsearc-ai/ready-set/blob/main/docs/contracts/change-log.md)
for the authoritative format.

## Cross-plugin composition

Plugins can call other plugins through the dispatcher (never directly), so
PATH semantics and built-in resolution stay consistent and the env contract
is forwarded:

```rust
use ready_set_sdk::dispatch;

let output = dispatch::run("scan", &["--json", "secrets"], &ctx)?;
```

The helper resolves the dispatcher binary via `which`, or via the
`READY_SET_BIN` env var when set (useful for tests and integration
scenarios).

## Stability

- The SDK's Rust API follows standard cargo semver within
  `ready-set-sdk`'s own version space.
- The contracts the SDK mirrors are tiered separately. See
  [`docs/contracts/README.md`](https://github.com/pulsearc-ai/ready-set/blob/main/docs/contracts/README.md)
  for which contracts are `stable` vs `experimental`.
- `#[non_exhaustive]` enums: `Error`, `ExitCode`, `DispatchOutcome`, and
  `sandbox::Capability`. Match these with a wildcard arm. Other enums
  (`CapabilityState`, `OutputMode`, `Stability`, `Platform`, `ChangeOp`, …)
  are intentionally exhaustive — they are pinned to versioned wire
  contracts, so adding a variant is a contract bump.
- The `sandbox` trait is `stable` in surface but its implementations are
  `experimental` and will gain real enforcement post-v0.1.0.

### Public dependency surface

The SDK re-exports some types from its own dependencies; those deps are
part of the SDK's public API and follow these rules:

- **`semver::Version`** (from `semver = 1.x`) is exposed in `Describe`,
  `Manifest`, and `Context::dispatcher_version()`. This is intentional —
  `semver::Version` is the de-facto Rust representation and wrapping it
  would force every plugin author to learn a parallel API. Bumping the
  `semver` major requirement is a major SDK bump.
- **`toml::Value`** (from `toml = 0.8.x`) currently appears in
  `Config.plugins`. This is **not** intentional and will be replaced
  before `0.1.0` stable with an opaque `PluginSection` wrapper so the
  underlying representation becomes a private implementation detail.
  Pre-`0.1.0` SDK releases may change this without further notice.

## Testing

```text
cargo test -p ready-set-sdk
```

`tests/contract_smoke.rs` exercises the contract types end-to-end against
the published examples in `docs/contracts/`.

## See also

- Workspace [`README.md`](https://github.com/pulsearc-ai/ready-set/blob/main/README.md) —
  product, lifecycle, plugin authoring overview.
- [`docs/contracts/`](https://github.com/pulsearc-ai/ready-set/tree/main/docs/contracts) —
  the authoritative wire and API contracts the SDK mirrors.
- [`ready-set`](https://crates.io/crates/ready-set) — the dispatcher binary
  that exec's plugins.
- [`ready-set-rust`](https://crates.io/crates/ready-set-rust) — a worked
  example of a real capability provider built on this SDK.

## License

Licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
