# Contract: `.ready-set.toml`

| Field      | Value                                  |
|------------|----------------------------------------|
| Stability  | `stable`                               |
| Version    | 2                                      |
| File path  | `<project_root>/.ready-set.toml`       |

`.ready-set.toml` holds project-local configuration for `ready-set` and its
plugins. The dispatcher resolves it by walking upward from the current
working directory in the same way cargo discovers `Cargo.toml`. The first
hit wins.

The dispatcher exports the resolved path as
[`READY_SET_CONFIG_PATH`](env-vars.md) before exec'ing a plugin.

## Top-level shape

```toml
[ready-set]
schema_version = 2
profile = "rust-workspace"

[capabilities.workspace]
relevance = "required"
provider = "rust"

[scan]
# Configuration for the hypothetical `ready-set scan` plugin.
```

## Required keys

| Section + key                | Type    | Required | Description                                                                                              |
|------------------------------|---------|----------|----------------------------------------------------------------------------------------------------------|
| `[ready-set]`                | table   | yes      | Top-level meta table. Always present.                                                                    |
| `ready-set.schema_version`   | integer | yes      | Schema version of this file. The contract version is `2`. Must equal `2`.                                |
| `ready-set.profile`          | string  | yes      | Product profile used to interpret capability relevance.                                                  |

All other sections and keys are optional.

## Capability sections

Capabilities are configured under `[capabilities.<id>]`, where `<id>` is the
capability id from [`capabilities.md`](capabilities.md).

```toml
[capabilities.linting]
relevance = "required"
provider = "rust"
```

| Key         | Type   | Required | Description                                                                 |
|-------------|--------|----------|-----------------------------------------------------------------------------|
| `relevance` | string | no       | Override relevance: `required`, `optional`, or `not-needed`.                |
| `provider`  | string | no       | Override provider id, such as `rust`.                                       |

Unknown capability ids and unknown keys inside capability tables are tolerated
for forward compatibility.

## Per-plugin sections

Each subcommand owns a TOML table named after the subcommand:

- `[ready]`, `[set]`, or `[go]` for lifecycle built-ins.
- `[undo]` for the planned undo built-in once it is implemented.
- `[<name>]` for the plugin invoked as `ready-set <name>`.

Plugins SHOULD document the keys they understand in their own `--help`.
Plugins MUST tolerate unknown keys within their own section (forward
compatibility within a single plugin).

## Forward compatibility

- **Unknown sections produce warnings, not errors.** A plugin loading the
  config sees its own section plus a list of unrecognized sections, which
  it MAY surface as a warning. The dispatcher itself never errors on unknown
  plugin sections. `[capabilities]` is reserved for capability configuration
  and is not a plugin section.
- **Unknown keys within `[ready-set]` are warnings.** The reserved namespace
  is the `ready-set` section.
- **`ready-set.schema_version` is exact.** This pre-v0.1 contract rejects
  schema versions other than `2`.

## Worked example

A typical Rust workspace:

```toml
[ready-set]
schema_version = 2
profile = "rust-workspace"

[capabilities.workspace]
relevance = "required"
provider = "rust"

[capabilities.toolchain]
relevance = "required"
provider = "rust"

[capabilities.formatting]
relevance = "required"
provider = "rust"

[capabilities.linting]
relevance = "required"
provider = "rust"

[scan]
exclude = ["vendor/**", "**/node_modules/**"]
```

A minimal valid file:

```toml
[ready-set]
schema_version = 2
profile = "rust-workspace"
```
