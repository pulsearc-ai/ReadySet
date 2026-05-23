# ready-set-rust

**ReadySet — by [PulseArc](https://github.com/pulsearc-ai).**

The first-party `ready-set` provider for Rust workspace foundations.

This crate ships the `ready-set-rust` binary, a provider plugin discovered
on `PATH` by the [`ready-set`](https://crates.io/crates/ready-set)
dispatcher. It contributes four capabilities to the readiness matrix and
answers the lifecycle protocol (`__ready`, `__set`, `__go`) for each.

The provider id is `rust`. The plugin requires a Cargo workspace
(`requires_cargo_workspace = true`) — outside one, lifecycle calls return
`ExitCode::NotCargoWorkspace`.

For the lifecycle grammar and how providers participate in the dispatcher,
see the workspace root
[`README.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/README.md).

## Install

```text
cargo install ready-set-rust
```

After installation the plugin is automatically discovered by the dispatcher.

## Capabilities

| Capability | Verbs | Files / commands managed |
|------------|-------|--------------------------|
| `workspace` | `ready`, `set` | Cargo workspace shape, `.gitignore` ready-set block, `.ready-set.toml` |
| `toolchain` | `ready`, `set` | `rust-toolchain.toml` |
| `formatting` | `ready`, `set`, `go` | `rustfmt.toml`; `cargo fmt --check` |
| `linting` | `ready`, `set`, `go` | `clippy.toml`, workspace `[workspace.lints]`; `cargo clippy --workspace --all-targets` |

All four capabilities default to `relevance = "required"`. A project can
override that in `.ready-set.toml`:

```toml
[capabilities.linting]
relevance = "optional"
```

## Usage

Through the dispatcher (the normal path):

```text
ready-set ready              # whole-product matrix; this plugin contributes 4 rows
ready-set ready linting
ready-set set                # reconcile required capabilities (workspace + toolchain + formatting + linting)
ready-set set linting
ready-set set --dry-run      # plan only; writes nothing
ready-set set --force        # overwrite diverged managed files
ready-set go formatting      # cargo fmt --check
ready-set go linting         # cargo clippy --workspace --all-targets
```

Direct provider protocol (rarely needed; mostly useful for plugin authors
debugging the contract):

```text
ready-set-rust __describe
ready-set-rust __ready linting
ready-set-rust __set linting
ready-set-rust __go linting
```

## What `set` writes

`set` is the only verb that mutates the filesystem. The Rust provider's
`set` produces these files in idempotent fashion:

| File | Owned by capability |
|------|---------------------|
| `rust-toolchain.toml` | `toolchain` |
| `rustfmt.toml` | `formatting` |
| `clippy.toml` | `linting` |
| `[workspace.lints]` table in the root `Cargo.toml` | `linting` |
| `.gitignore` ready-set block | `workspace` |
| `.ready-set.toml` (schema v2) | `workspace` |

Templates live under [`src/templates/`](src/templates/) and are embedded at
compile time via `include_str!`.

`set` accepts:

- `--dry-run` — plan only; writes nothing.
- `--force` — overwrite managed files even if they have diverged.
- `--member <path>` — narrow scope to a single workspace member.
- `--no-discover` — skip workspace member discovery.

Every write is recorded under `.ready-set/changes/rust-<timestamp>-<rand>.jsonl`
with a `before_sha256` / `after_sha256` per record. Pre-mutation file
content is preserved under `.ready-set/backups/<sha256>` so the planned
`ready-set undo` can reverse it.

## What `go` runs

`go` never bootstraps missing files. It runs the canonical local workflow
for the capability and reports failure correctly:

| Capability | Command |
|------------|---------|
| `formatting` | `cargo fmt --check` |
| `linting` | `cargo clippy --workspace --all-targets` |
| `workspace` | not supported (no `go` verb in the descriptor) |
| `toolchain` | not supported |

Selecting a capability that does not support `go` is a user error rejected
by the dispatcher before the provider is spawned.

## Module map

```text
ready-set-rust/src/
├── main.rs              # binary entry; parses lifecycle request and dispatches
├── lib.rs               # describe() + rust_capabilities()
├── readiness.rs         # __ready: per-capability state evaluation
├── runner.rs            # __set: planning + writing through the change log
├── workflow.rs          # __go: cargo fmt / cargo clippy invocation
├── workspace.rs         # Cargo workspace resolution (walks upward)
├── members.rs           # Cargo workspace member discovery
├── manifest_edit.rs     # toml_edit-based root Cargo.toml updates
├── ready_set_toml.rs    # generates .ready-set.toml (schema v2)
├── gitignore.rs         # manage the .gitignore ready-set block
├── options.rs           # CLI argument parsing for __set
├── templates.rs         # include_str! handles for the embedded templates
└── templates/
    ├── rust-toolchain.toml
    ├── rustfmt.toml
    ├── clippy.toml
    ├── workspace-lints.toml
    └── gitignore
```

`tests/rust_provider_e2e.rs` runs the binary against fresh and dirty
fixture workspaces to verify idempotency, `--dry-run`, `--force`, change
log records, and the workflow exit codes.

## Library surface

`ready-set-rust` is primarily a binary, but exposes a library so the
dispatcher's integration tests and downstream tooling can drive it
in-process:

```rust
pub const PROVIDER_ID: &str = "rust";

pub fn describe() -> ready_set_sdk::describe::Describe;
pub fn rust_capabilities() -> Vec<ready_set_sdk::CapabilityDescriptor>;
```

The submodules (`readiness`, `runner`, `workflow`, `workspace`, …) are
public for testing and embedding but are not a stable embedder API. The
stable surface is the CLI plus the lifecycle protocol.

## Reversibility

Every `set` write is recorded as a `ChangeRecord` JSONL line under
`.ready-set/changes/rust-<timestamp>-<rand>.jsonl`:

```json
{"op":"create","path":"rustfmt.toml","before_sha256":null,"after_sha256":"…"}
```

When a managed file is modified or deleted, pre-change content is saved
under `.ready-set/backups/<sha256>` (content-addressed). The planned
`ready-set undo` will consume these records regardless of which provider
produced them.

`set --dry-run` writes neither change records nor backups.

See
[`docs/contracts/change-log.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/change-log.md).

## Platform support

`platforms = ["linux", "macos", "windows"]`. The provider invokes Cargo via
`std::process::Command` (no `sh -c`, no `cmd /c`) so the same code path
runs on all three platforms. Path handling uses `std::path::Path` /
`PathBuf` throughout.

## See also

- Workspace [`README.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/README.md) —
  product, lifecycle, principles, roadmap.
- [`docs/contracts/`](https://github.com/pulsearc-ai/ReadySet/tree/main/docs/contracts) —
  versioned protocol specs the provider conforms to.
- [`ready-set`](https://crates.io/crates/ready-set) — the dispatcher that
  discovers and exec's this provider.
- [`ready-set-sdk`](https://crates.io/crates/ready-set-sdk) — the SDK this
  provider builds on; also the recommended starting point for writing your
  own provider.

## License

Licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE), at your option.
