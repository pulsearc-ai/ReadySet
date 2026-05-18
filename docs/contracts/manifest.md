# Contract: Plugin manifest sidecar

| Field      | Value                                                |
|------------|------------------------------------------------------|
| Stability  | `stable`                                             |
| Version    | 1                                                    |
| File name  | `ready-set-<name>.toml` next to the plugin binary    |
| Schema     | [`schemas/manifest.schema.json`](schemas/manifest.schema.json) (Draft 2020-12) |

A plugin SHOULD ship a manifest sidecar alongside its binary on PATH. The
dispatcher reads it without spawning the plugin, which keeps `ready-set --list`
fast and side-effect free. When the sidecar is absent, the dispatcher falls
back to the [`__describe`](describe.md) subcommand.

## Discovery

For a plugin binary at `<dir>/ready-set-<name>` (or `<dir>/ready-set-<name>.exe`
on Windows), the dispatcher looks for `<dir>/ready-set-<name>.toml`. The
sidecar MUST be in the same directory as the binary it describes.

## Schema

```toml
description            = "one-line summary, max 80 chars"
version                = "1.2.3"
stability              = "stable"            # one of: stable | experimental | deprecated
min_dispatcher_version = "0.1.0"
platforms              = ["linux", "macos", "windows"]
requires_cargo_workspace = false
capabilities = []
```

| Key                        | Type     | Required | Description                                                                                                                                                       |
|----------------------------|----------|----------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `description`              | string   | yes      | One-line summary shown in `ready-set --list`. Maximum 80 characters. No newlines.                                                                                 |
| `version`                  | string   | yes      | Semver version of the plugin binary. Used for diagnostics and bug reports.                                                                                        |
| `stability`                | string   | yes      | Stability tier. One of `stable`, `experimental`, `deprecated`.                                                                                                    |
| `min_dispatcher_version`   | string   | yes      | Minimum dispatcher semver this plugin requires. The dispatcher emits a warning (not an error) if the running core is older. Plugins SHOULD pick the lowest version they actually need. |
| `platforms`                | array    | yes      | OS names this plugin supports. One or more of `linux`, `macos`, `windows`. The dispatcher hides plugins from `--list` whose platforms exclude the current OS unless `--all` is passed. |
| `requires_cargo_workspace` | boolean  | yes      | Whether the plugin requires the dispatcher to invoke it inside a cargo workspace. Informational; the dispatcher does not enforce.                                  |
| `capabilities`             | array    | yes      | Capability descriptors contributed by this plugin. Use `[]` when the plugin exposes no lifecycle capabilities. See [`capabilities.md`](capabilities.md).           |

## Forward compatibility

- Plugins MAY include additional keys not listed above. The dispatcher
  ignores unknown keys.
- Adding a new optional key in a future minor version of this contract is
  not a breaking change.

## Capability metadata

Every manifest MUST include `capabilities`. This is a breaking pre-v0.1
contract change that makes lifecycle participation explicit for every plugin.

```toml
[[capabilities]]
id = "linting"
title = "Linting"
provider = "rust"
verbs = ["ready", "set", "go"]
default_relevance = "required"
```

Plugins that do not provide lifecycle capabilities still include the key:

```toml
capabilities = []
```

## Worked examples

### A first-party plugin (full manifest)

```toml
description              = "Scaffold a project skill from a template"
version                  = "0.2.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
requires_cargo_workspace = false
capabilities             = []
```

### A Linux-only experimental plugin

```toml
description              = "Inspect ELF binaries for unsafe linkage"
version                  = "0.0.3"
stability                = "experimental"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux"]
requires_cargo_workspace = false
capabilities             = []
```

### A plugin without a sidecar

If `ready-set-foo.toml` is missing, the dispatcher falls back to running
`ready-set-foo __describe`. See [`describe.md`](describe.md). Plugins
distributed via `cargo install` typically rely on the fallback because
`cargo install` does not place sidecar files; tarball/Homebrew distributions
should ship the sidecar alongside the binary.
