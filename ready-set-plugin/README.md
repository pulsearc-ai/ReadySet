# ready-set-plugin

YAML-driven scaffold generator for `ready-set` plugin crates.

This binary is itself a normal plugin: install it on `PATH` as
`ready-set-plugin`, then invoke it through the dispatcher as `ready-set
plugin ...`.

## Quick Start

```text
ready-set plugin new scan --kind provider --capability policy-scan --verbs ready,go
cd ready-set-scan
cargo test
```

For command-only plugins:

```text
ready-set plugin new template --kind command
```

For local development against a checked-out SDK:

```text
ready-set plugin new scan \
  --sdk-path /path/to/ready-set/ready-set-sdk \
  --verify
```

## YAML First

Create a starter blueprint:

```text
ready-set plugin init scan > ready-set-plugin.yaml
```

Validate it before generation:

```text
ready-set plugin validate ready-set-plugin.yaml
```

Generate or regenerate:

```text
ready-set plugin generate ready-set-plugin.yaml --force
```

From inside an existing generated crate, regenerate in place:

```text
ready-set plugin generate ready-set-plugin.yaml --path . --force
```

The blueprint contract is documented in
[`docs/contracts/plugin-blueprint.md`](../docs/contracts/plugin-blueprint.md);
the editor/tooling schema lives at
[`docs/contracts/schemas/plugin-blueprint.schema.json`](../docs/contracts/schemas/plugin-blueprint.schema.json).

## Generated Files

The generated crate includes:

- `Cargo.toml` with a `ready-set-sdk` dependency.
- `src/main.rs` as a thin generated entry point.
- `src/generated/describe.rs` with static metadata, capabilities, aliases,
  and project requirements.
- `src/generated/config.rs` with typed `.ready-set.toml` plugin-section
  loading.
- `src/generated/routing.rs` with `__describe` and lifecycle routing.
- `src/handlers/ready.rs`, `src/handlers/set.rs`, and `src/handlers/go.rs`
  handler stubs for user code.
- `tests/contract.rs` proving metadata and lifecycle wire shape.
- Optional `dist/ready-set-<name>.toml` sidecar manifest.

Regeneration overwrites generated files and preserves handler files, even
with `--force`.

The generator formats the crate with `cargo fmt`; `--verify` also runs
`cargo fmt --check` and `cargo test`.
