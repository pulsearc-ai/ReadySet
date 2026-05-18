# AGENTS.md

Guidance for future agents working in this repository.

## Project Model

`ready-set` is a product-readiness CLI. It models a repository as a set of
capabilities such as `workspace`, `toolchain`, `formatting`, `linting`,
`tests`, `ci`, `release`, `deploy`, and `docs`.

Every capability follows the same lifecycle:

```text
ready -> set -> go
```

- `ready` is read-only diagnosis. It answers whether a capability can be used.
- `set` creates or reconciles missing or stale capability setup.
- `go` executes the capability's main workflow.

The core product is a framework and dispatcher, not a Rust bootstrap command.
Rust-specific behavior belongs to the first-party `ready-set-rust` provider
plugin.

## Current Architecture

- `ready-set/` is the core dispatcher.
  - Owns CLI parsing, built-ins, plugin discovery, capability registry,
    lifecycle dispatch, env export, and matrix rendering.
  - Built-ins are `ready`, `set`, `go`, `help`, `list`, and `version`.
  - Bare `ready-set` routes to `ready` and shows the readiness matrix.
- `ready-set-sdk/` is the shared contract and helper crate.
  - Owns typed capability descriptors/reports/run reports, manifests,
    `.ready-set.toml` v2 parsing, lifecycle request parsing, output helpers,
    change-log helpers, and exit codes.
- `ready-set-rust/` is the first-party Rust capability provider.
  - Provider id: `rust`.
  - Capabilities: `workspace`, `toolchain`, `formatting`, `linting`.
  - `workspace` and `toolchain` support `ready` and `set`.
  - `formatting` and `linting` support `ready`, `set`, and `go`.
- `docs/contracts/` is the public protocol surface for plugin authors.
- `README.md` is the architecture narrative, design principles, roadmap to
  v0.1.0, and product direction in one document.

## Non-Negotiable Rules

- Do not reintroduce core Rust bootstrap behavior.
  - `ready-set go` must not create `rust-toolchain.toml`, `rustfmt.toml`,
    `clippy.toml`, `.ready-set.toml`, or change logs.
  - Setup belongs to `ready-set set`, dispatched to provider plugins.
- Keep core generic.
  - Domain knowledge belongs in provider plugins.
  - The core discovers capability descriptors and dispatches protocol calls.
- Treat plugin metadata as the registry source.
  - Capabilities come from manifest sidecars or `__describe`.
  - `.ready-set.toml` can override relevance and provider selection, but it
    does not invent available capabilities by itself.
- Preserve read-only behavior.
  - `ready` and `go` should not mutate tracked project files.
  - `set --dry-run` must not write files or change logs.
- Preserve mutation safety.
  - Provider `set` mutations should record JSONL change logs under
    `.ready-set/changes/`.
  - Modified files need backups under `.ready-set/backups/`.
  - `undo` is planned, not currently routed as an available built-in.
- Keep public contracts stable unless the task explicitly changes them.
  - Contract changes usually require docs, SDK types, schemas, and tests.

## Lifecycle Dispatch Contract

For a capability descriptor with provider id `foo`, core invokes:

```text
ready-set-foo __ready <capability>
ready-set-foo __set   <capability> [args...]
ready-set-foo __go    <capability> [args...]
```

Provider commands receive the `READY_SET_*` env contract. Core strips unknown
incoming `READY_SET_*` variables before invoking providers.

Expected output:

- `__ready` emits one JSON `CapabilityReport`.
- `__set` and `__go` emit `CapabilityRunReport` in JSON mode.
- Human mode may stream provider output for `set` and `go`.
- Unsupported lifecycle verbs are rejected by core before provider execution.

## Where To Make Changes

- Core command behavior: `ready-set/src/builtins/`,
  `ready-set/src/capabilities.rs`, `ready-set/src/lifecycle.rs`,
  `ready-set/src/lib.rs`.
- Plugin discovery and metadata: `ready-set/src/discovery.rs`,
  `ready-set/src/metadata.rs`, `ready-set/src/cache.rs`.
- SDK contract types: `ready-set-sdk/src/capability.rs`,
  `ready-set-sdk/src/manifest.rs`, `ready-set-sdk/src/describe.rs`,
  `ready-set-sdk/src/config.rs`, `ready-set-sdk/src/lifecycle.rs`.
- Rust provider setup: `ready-set-rust/src/runner.rs`,
  `ready-set-rust/src/readiness.rs`, `ready-set-rust/src/workflow.rs`,
  `ready-set-rust/src/manifest_edit.rs`.
- Public contract docs: `docs/contracts/`.

If a change crosses a contract boundary, update the docs and tests in the same
change.

## Testing Expectations

Run the smallest useful test first, then broaden before finishing.

Common commands:

```text
cargo fmt --check
cargo test -p ready-set-sdk
cargo test -p ready-set
cargo test -p ready-set-rust
cargo test --workspace
cargo clippy --workspace --all-targets
```

Useful stale-wording scan when touching lifecycle docs or command behavior:

```text
rg -n 'Bootstrap[[:space:]]+a[[:space:]]+Rust[[:space:]]+workspace|bootstrap[ -]built[ -]in|ready-set[[:space:]]+go.*bootstraps?|original[[:space:]]+bootstrap[[:space:]]+value|pending .*ready-set[[:space:]]+go|bootstrap[[:space:]]+change[[:space:]]+logs' ready-set ready-set-rust ready-set-sdk docs README.md
```

## Agent Workflow

Before editing:

1. Read the relevant contract or design section.
2. Check the current worktree and avoid overwriting unrelated user changes.
3. Identify whether the change belongs in core, SDK, provider, or docs.

While editing:

- Keep changes scoped to the requested step or feature.
- Prefer existing local patterns over new abstractions.
- Use provider plugins for domain behavior.
- Do not advertise commands in help/list until they are actually routed.
- Keep JSON output compatible with SDK report shapes.

Before handing off:

- Run the relevant tests and report any command that could not be completed.
- Mention any contract/docs updates made.
- Note any remaining stale references if they are intentionally outside scope.
