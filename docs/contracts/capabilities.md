# Contract: Capabilities

| Field     | Value                                                                                      |
|-----------|--------------------------------------------------------------------------------------------|
| Stability | `stable`                                                                                   |
| Version   | 1                                                                                          |
| Schemas   | [`schemas/capability-descriptor.schema.json`](schemas/capability-descriptor.schema.json), [`schemas/capability-report.schema.json`](schemas/capability-report.schema.json), [`schemas/capability-run-report.schema.json`](schemas/capability-run-report.schema.json) |

Capabilities are product concerns that can be checked, configured, and
executed through the lifecycle verbs:

```text
ready
set
go
```

The dispatcher uses capability metadata to build the product readiness matrix
and to route lifecycle commands to plugin providers.

## Capability descriptor

A descriptor is static metadata declared by a provider plugin manifest /
`__describe` payload.

```json
{
  "id": "formatting",
  "title": "Formatting",
  "provider": "rust",
  "verbs": ["ready", "set", "go"],
  "default_relevance": "required"
}
```

| Key                 | Type   | Required | Description                                                     |
|---------------------|--------|----------|-----------------------------------------------------------------|
| `id`                | string | yes      | Stable capability id. Lowercase kebab-case: `^[a-z][a-z0-9-]*$`. |
| `title`             | string | yes      | Human label for matrix and help output.                         |
| `provider`          | string | yes      | Stable provider id, usually the plugin subcommand.              |
| `verbs`             | array  | yes      | Non-empty unique list of supported verbs: `ready`, `set`, `go`. |
| `default_relevance` | string | yes      | Default product relevance: `required`, `optional`, or `not-needed`. |

## Capability report

A report is the read-only status row emitted by `ready-set ready` and by
capability providers.

```json
{
  "id": "linting",
  "title": "Linting",
  "provider": "rust",
  "state": "missing",
  "relevance": "required",
  "summary": "clippy.toml is missing",
  "next_action": {
    "command": "ready-set set linting",
    "description": "Create the linting configuration"
  }
}
```

| Key           | Type          | Required | Description                                                                 |
|---------------|---------------|----------|-----------------------------------------------------------------------------|
| `id`          | string        | yes      | Capability id matching its descriptor.                                       |
| `title`       | string        | yes      | Human label.                                                                |
| `provider`    | string        | yes      | Provider id.                                                                |
| `state`       | string        | yes      | One of `ready`, `missing`, `incomplete`, `blocked`, `stale`, `optional`, `not-needed`. |
| `relevance`   | string        | yes      | Effective product relevance: `required`, `optional`, or `not-needed`.       |
| `summary`     | string        | yes      | Short explanation of the current state.                                      |
| `next_action` | object/null   | yes      | Suggested next command, or `null` when no action is needed.                  |

`next_action`, when present, has this shape:

```json
{
  "command": "ready-set set linting",
  "description": "Create the linting configuration"
}
```

## Capability run report

A run report is emitted by lifecycle verbs that perform work: `set` and `go`.

```json
{
  "id": "linting",
  "verb": "set",
  "status": "changed",
  "actions": [
    {
      "path": "clippy.toml",
      "kind": "create",
      "summary": "created clippy config"
    }
  ]
}
```

| Key       | Type   | Required | Description                                                |
|-----------|--------|----------|------------------------------------------------------------|
| `id`      | string | yes      | Capability id.                                             |
| `verb`    | string | yes      | Lifecycle verb that ran: `set` or `go`.                    |
| `status`  | string | yes      | One of `ok`, `changed`, `noop`, or `failed`.               |
| `actions` | array  | yes      | Ordered actions checked, skipped, executed, or failed.     |

Each action has:

| Key       | Type   | Required | Description                                                                 |
|-----------|--------|----------|-----------------------------------------------------------------------------|
| `kind`    | string | yes      | One of `create`, `modify`, `delete`, `run`, `check`, `skip`, or `error`.     |
| `summary` | string | yes      | Short action summary.                                                       |
| `path`    | string | no       | Project-relative path when the action concerns a filesystem path.           |

## Manifest and `__describe` integration

Plugin metadata MUST include a `capabilities` array. Use an empty array when a
plugin exposes no lifecycle capabilities.

```json
{
  "description": "Rust product capabilities",
  "version": "0.1.0",
  "stability": "stable",
  "min_dispatcher_version": "0.1.0",
  "platforms": ["linux", "macos", "windows"],
  "project_requirements": ["cargo-workspace"],
  "capabilities": [
    {
      "id": "linting",
      "title": "Linting",
      "provider": "rust",
      "verbs": ["ready", "set", "go"],
      "default_relevance": "required"
    }
  ]
}
```

The `capabilities` field is required by the manifest and `__describe`
contracts. This is a breaking pre-v0.1 contract change so every plugin declares
whether it participates in the lifecycle model.

## Lifecycle provider protocol

For a capability descriptor with provider id `foo`, the dispatcher invokes the
provider binary as `ready-set-foo` and passes lifecycle protocol subcommands:

```text
ready-set-foo __ready <capability>
ready-set-foo __set <capability> [args...]
ready-set-foo __go <capability> [args...]
```

`__ready` MUST be read-only and emit one JSON `CapabilityReport` on stdout.
`__set` and `__go` perform work and SHOULD emit one JSON
`CapabilityRunReport` when `READY_SET_OUTPUT=json`; otherwise they MAY print
human-oriented progress. A failed `__go` workflow may still emit a valid
failed run report before exiting nonzero. The dispatcher rejects unsupported
lifecycle verbs before spawning the provider.
