# Contract: Dispatcher → plugin env vars

| Field | Value |
|---|---|
| Stability | `stable` |
| Version | 1 |
| Reserved namespace | `READY_SET_*` |

The dispatcher exports a fixed set of environment variables before exec'ing
(or spawning, on Windows) a plugin. Plugins read these instead of
re-resolving project state. Third-party plugins should treat this set as the
canonical source of truth for the invocation context.

## Variables

| Name | Format | Required | Meaning |
|---|---|---|---|
| `READY_SET_DISPATCHER_VERSION` | semver, e.g. `0.1.0` | Always set when the dispatcher invokes a plugin. | Version of the `ready-set` core that produced this invocation. |
| `READY_SET_PROJECT_ROOT` | absolute path | Set when a project root was resolved; absent otherwise. | Absolute, canonicalized path to the resolved project root (the directory containing `.git`, the nearest `Cargo.toml`, or `.ready-set.toml`, in that order of preference). |
| `READY_SET_CONFIG_PATH` | absolute path | Set when `.ready-set.toml` was found; absent otherwise. | Absolute, canonicalized path to the resolved `.ready-set.toml` for this invocation. |
| `READY_SET_OUTPUT` | one of `human`, `json` | Always set. Defaults to `human`. | Requested output mode. Plugins must honor this. |
| `READY_SET_LOG` | one of `quiet`, `normal`, `verbose` | Always set. Defaults to `normal`. | Requested log verbosity. |
| `READY_SET_COLOR` | one of `auto`, `always`, `never` | Always set. Defaults to `auto`. | Color preference. `auto` means: emit ANSI escape codes only if stdout is a TTY. |

## Forward compatibility rules

1. **Tolerate unset.** If a variable is absent, the plugin SHOULD fall back
   to a documented default (typically `auto`/`normal`/`human`/cwd) without
   erroring.
2. **Tolerate unrecognized values.** If `READY_SET_OUTPUT` is `yaml` (a
   future addition), a plugin that only knows `human|json` SHOULD fall back
   to `human` and continue. Treat unrecognized values as if the variable
   were absent.
3. **Never crash on extras.** The dispatcher reserves the entire
   `READY_SET_*` namespace. Plugins MUST NOT error if they encounter
   `READY_SET_FOO` they don't recognize. Future additions are minor changes.
4. **Paths are absolute.** Plugins MUST NOT receive relative paths. The
   dispatcher canonicalizes paths before exporting them.

## Reserved namespace

The dispatcher reserves the entire `READY_SET_*` environment variable
namespace. Plugins MUST NOT define their own variables in this namespace.
The dispatcher MAY clear unknown `READY_SET_*` variables from the parent
environment before exec'ing a plugin to prevent injection from the calling
shell.

## Plugin obligation

Plugins MUST NOT mutate `READY_SET_*` variables for child processes they
spawn unless they also re-export the full set as the dispatcher would. The
SDK's `dispatch()` helper handles this correctly; manual `Command::new`
invocations should set the env explicitly.

## Worked example

A plugin invocation as `ready-set scan --json` from
`/Users/me/code/myproj/src` (a cargo workspace containing `.ready-set.toml`
at its root) sees:

```text
READY_SET_DISPATCHER_VERSION=0.1.0
READY_SET_PROJECT_ROOT=/Users/me/code/myproj
READY_SET_CONFIG_PATH=/Users/me/code/myproj/.ready-set.toml
READY_SET_OUTPUT=json
READY_SET_LOG=normal
READY_SET_COLOR=auto
```

A plugin invocation outside any project (e.g., from `/tmp`):

```text
READY_SET_DISPATCHER_VERSION=0.1.0
READY_SET_OUTPUT=human
READY_SET_LOG=normal
READY_SET_COLOR=auto
```

(`READY_SET_PROJECT_ROOT` and `READY_SET_CONFIG_PATH` absent.)
