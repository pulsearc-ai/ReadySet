# Contract: Plugin manifest sidecar

| Field      | Value                                                |
|------------|------------------------------------------------------|
| Stability  | `stable`                                             |
| Version    | 2                                                    |
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
project_requirements   = []                  # optional; e.g. ["cargo-workspace"]
capabilities           = []
command_aliases        = []                  # optional; see "Command aliases" below
```

| Key                        | Type     | Required | Description                                                                                                                                                       |
|----------------------------|----------|----------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `description`              | string   | yes      | One-line summary shown in `ready-set --list`. Maximum 80 characters. No newlines.                                                                                 |
| `version`                  | string   | yes      | Semver version of the plugin binary. Used for diagnostics and bug reports.                                                                                        |
| `stability`                | string   | yes      | Stability tier. One of `stable`, `experimental`, `deprecated`.                                                                                                    |
| `min_dispatcher_version`   | string   | yes      | Minimum dispatcher semver this plugin requires. The dispatcher emits a warning (not an error) if the running core is older. Plugins SHOULD pick the lowest version they actually need. |
| `platforms`                | array    | yes      | OS names this plugin supports. One or more of `linux`, `macos`, `windows`. The dispatcher hides plugins from `--list` whose platforms exclude the current OS unless `--all` is passed. |
| `project_requirements`     | array    | no       | Named project requirements the plugin advertises, e.g. `["cargo-workspace"]`. Optional; omit or use `[]` when none. Informational; the dispatcher does not enforce. Replaces the v1 `requires_cargo_workspace` boolean.                                  |
| `capabilities`             | array    | yes      | Capability descriptors contributed by this plugin. Use `[]` when the plugin exposes no lifecycle capabilities. See [`capabilities.md`](capabilities.md).           |
| `command_aliases`          | array    | no       | User-facing `ready-set <name>` commands contributed by this plugin. Optional; omit or use `[]` when none. See [Command aliases](#command-aliases).                |

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

## Command aliases

A plugin MAY contribute user-facing `ready-set <name>` commands so short verbs
route to a capability lifecycle verb or to the plugin binary, without hardcoding
provider-specific knowledge in the dispatcher. Aliases are optional; omit the
key or use `command_aliases = []` when the plugin declares none.

> **Status:** the manifest and `__describe` carry `command_aliases` as of
> contract v2, and the SDK exposes the `CommandAlias` type. The dispatcher
> resolves aliases before falling through to same-named plugin subcommands and
> displays discovered aliases in `ready-set --list`. Older dispatchers that do
> not understand the key ignore it (the manifest schema is
> `additionalProperties: true`).

```toml
[[command_aliases]]
name        = "encrypt"
description = "Encrypt configured dotenv files into bundles."
target      = "set"
capability  = "secret-bundles"

[[command_aliases]]
name            = "encrypt"
description     = "Show configured bundles and redacted keys."
match_first_arg = "status"
target          = "plugin"
args            = ["bundle"]

[[command_aliases]]
name        = "rotate"
description = "Rotate or record reminders for due secrets."
target      = "go"
capability  = "rotation"
```

| Key               | Type   | Required        | Description                                                                                                                                          |
|-------------------|--------|-----------------|------------------------------------------------------------------------------------------------------------------------------------------------------|
| `name`            | string | yes             | Command typed after `ready-set`. Lowercase kebab-case (`^[a-z][a-z0-9-]*$`).                                                                          |
| `description`     | string | yes             | One-line summary shown in `ready-set --list`. Maximum 80 characters, no newlines.                                                                    |
| `match_first_arg` | string | no              | When set, the alias matches only if the user's first argument equals this value. Lets one `name` fan out by sub-verb; the most specific match wins.   |
| `target`          | string | yes             | Dispatch target. One of `set`, `go`, `plugin`.                                                                                                        |
| `capability`      | string | with `set`/`go` | Capability id passed to the provider lifecycle verb. Required when `target` is `set` or `go`; not allowed when `target` is `plugin`.                  |
| `args`            | array  | `plugin` only   | Arguments prepended before the user's alias arguments. Only allowed when `target` is `plugin`.                                                        |

Multiple aliases MAY share a `name`: pair one bare alias (no `match_first_arg`)
with arg-specific aliases to route sub-verbs under one command. The dispatcher
selects the most specific matching alias. See
[`schemas/command-alias.schema.json`](schemas/command-alias.schema.json).

## Worked examples

### A first-party plugin (full manifest)

```toml
description              = "Scaffold a project skill from a template"
version                  = "0.2.0"
stability                = "stable"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux", "macos", "windows"]
capabilities             = []
```

### A Linux-only experimental plugin

```toml
description              = "Inspect ELF binaries for unsafe linkage"
version                  = "0.0.3"
stability                = "experimental"
min_dispatcher_version   = "0.1.0"
platforms                = ["linux"]
capabilities             = []
```

### A plugin without a sidecar

If `ready-set-foo.toml` is missing, the dispatcher falls back to running
`ready-set-foo __describe`. See [`describe.md`](describe.md). Plugins
distributed via `cargo install` typically rely on the fallback because
`cargo install` does not place sidecar files; tarball/Homebrew distributions
should ship the sidecar alongside the binary.

## Changes in version 2

- Replaced the required `requires_cargo_workspace` boolean with the optional
  `project_requirements` string array. Use `["cargo-workspace"]` for the old
  `true`; omit or use `[]` for the old `false`. The field remains
  informational — the dispatcher does not enforce it. This is a breaking change
  to the manifest shape, made before v0.1.0.
- Added the optional `command_aliases` array (see
  [Command aliases](#command-aliases)).
