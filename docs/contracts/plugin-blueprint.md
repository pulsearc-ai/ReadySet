# Contract: Plugin Blueprint

| Field     | Value          |
|-----------|----------------|
| Stability | `experimental` |
| Version   | 1              |

`ready-set-plugin` consumes a YAML blueprint to generate or regenerate a
plugin crate. The blueprint is a source file owned by the plugin author; it is
not read by the dispatcher at runtime.

## Top-Level Shape

```yaml
schema_version: 1
plugin:
  name: scan
  description: Scan project files
  version: 0.1.0
  stability: experimental
  min_dispatcher_version: 0.1.0
capabilities:
  - id: policy-scan
    title: Policy Scan
    verbs: [ready, go]
```

| Key              | Type   | Required | Description                                                |
|------------------|--------|----------|------------------------------------------------------------|
| `schema_version` | number | yes      | Must be `1`.                                               |
| `plugin`         | object | yes      | Package, binary, metadata, platform, and requirements.     |
| `capabilities`   | array  | no       | Capability descriptors and generated handler hints.        |
| `aliases`        | array  | no       | Provider-declared `ready-set <name>` aliases.              |
| `config`         | object | no       | Generated `.ready-set.toml` plugin-section loader shape.   |
| `dependencies`   | object | no       | Declared external tools, network, and file-write behavior. |
| `generation`     | object | no       | Generator options for optional files and SDK dependency.   |

Unknown keys are rejected.

## Schema

The JSON Schema for editor tooling and CI validation lives at
[`schemas/plugin-blueprint.schema.json`](schemas/plugin-blueprint.schema.json).

