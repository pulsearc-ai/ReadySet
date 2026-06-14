# ready-set Contracts

**ReadySet — by [PulseArc](https://github.com/pulsearc-ai).**

This directory holds the long-term contracts that bind the `ready-set`
ecosystem. Every plugin — first-party or third-party, written in Rust or any
other language — depends on these surfaces. They are designed to be stable for
the lifetime of `ready-set 0.x.y`.

## Stability tiers

| Tier                    | Meaning                                                                                                                                                    |
|-------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `stable`                | Frozen at v0.1.0. Future minor versions can add fields, never remove or change semantics. Breaking changes require a major-version bump of the dispatcher. |
| `experimental`          | Subject to change before the next minor release. Plugins relying on these must accept breakage.                                                            |
| `experimental-internal` | Not part of the public ecosystem surface. Versioned only so that future rollouts are clean.                                                                |

## Contracts

| Spec                                     | Tier                    | Summary                                                                                                                                                 |
|------------------------------------------|-------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------|
| [`env-vars.md`](env-vars.md)             | `stable`                | `READY_SET_*` env vars the dispatcher exports before exec'ing a plugin.                                                                                 |
| [`capabilities.md`](capabilities.md)     | `stable`                | Product capability descriptors, readiness reports, and lifecycle run reports.                                                                           |
| [`manifest.md`](manifest.md)             | `stable`                | TOML schema for the `ready-set-<name>.toml` plugin manifest sidecar.                                                                                    |
| [`describe.md`](describe.md)             | `stable`                | The `__describe` subcommand contract used as a fallback when no sidecar manifest is present.                                                            |
| [`change-log.md`](change-log.md)         | `stable`                | JSONL grammar for the per-project change log under `.ready-set/changes/`. Intended for the planned `ready-set undo` command regardless of which plugin produced the records. |
| [`ready-set-toml.md`](ready-set-toml.md) | `stable`                | Schema for the project-local `.ready-set.toml`.                                                                                                         |
| [`exit-codes.md`](exit-codes.md)         | `stable`                | Process exit codes returned by every `ready-set` command (built-ins and plugins).                                                                       |
| [`sdk-api.md`](sdk-api.md)               | `stable`                | Public Rust API surface of the `ready-set-sdk` crate at v0.1.0.                                                                                         |
| [`plugin-blueprint.md`](plugin-blueprint.md) | `experimental`      | YAML blueprint consumed by `ready-set-plugin` to scaffold a plugin crate.                                                                               |
| [`cache.md`](cache.md)                   | `experimental-internal` | Schema for the dispatcher's `--list` cache file.                                                                                                        |

## JSON Schemas

Machine-readable validators live under [`schemas/`](schemas/). They are
JSON Schema **Draft 2020-12** documents.

| Schema                                                                   | Validates                                                     |
|--------------------------------------------------------------------------|---------------------------------------------------------------|
| [`schemas/manifest.schema.json`](schemas/manifest.schema.json)           | TOML-decoded sidecar manifest objects.                        |
| [`schemas/describe.schema.json`](schemas/describe.schema.json)           | One-line JSON output of `__describe`.                         |
| [`schemas/capability-descriptor.schema.json`](schemas/capability-descriptor.schema.json) | Static metadata for one product capability.                   |
| [`schemas/command-alias.schema.json`](schemas/command-alias.schema.json) | A provider-declared `ready-set <name>` command alias.         |
| [`schemas/plugin-blueprint.schema.json`](schemas/plugin-blueprint.schema.json) | YAML-decoded `ready-set-plugin` blueprint objects.            |
| [`schemas/capability-report.schema.json`](schemas/capability-report.schema.json) | Read-only readiness status for one product capability.        |
| [`schemas/capability-run-report.schema.json`](schemas/capability-run-report.schema.json) | Structured result from running a capability lifecycle verb.   |
| [`schemas/change-record.schema.json`](schemas/change-record.schema.json) | A single JSONL line in `.ready-set/changes/<plugin>-*.jsonl`. |
| [`schemas/cache.schema.json`](schemas/cache.schema.json)                 | The `plugins.json` cache file produced by the dispatcher.     |

## Semver policy

The "stable" contracts above bind every plugin in the ecosystem. Within
`ready-set 0.x.y`:

- **Adding** a new optional field to a stable contract is a *minor* change.
- **Adding** a new optional `READY_SET_*` env var is a *minor* change.
- **Removing** a field, **changing** the semantics of an existing field, or
  **renaming** anything is a *breaking* change for the dispatcher and requires
  a major-version bump.

Plugin authors should treat the stable contracts as a forward-compatible
surface: tolerate unknown fields, ignore unrecognized values, and never
hardcode field counts.

The SDK's Rust API ([`sdk-api.md`](sdk-api.md)) follows standard cargo semver
within `ready-set-sdk`'s own version space, independently of the dispatcher.
