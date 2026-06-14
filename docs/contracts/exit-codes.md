# Contract: Exit codes

| Field     | Value    |
|-----------|----------|
| Stability | `stable` |
| Version   | 2        |

Every `ready-set` command — built-in or plugin — returns a process exit code
from a fixed set. The dispatcher's own meta-commands (`--list`, `--help`,
`--version`) follow the same conventions.

## Code table

| Code  | Constant                | Meaning                                                                                                                                                              |
|-------|-------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `0`   | `OK`                    | Success.                                                                                                                                                             |
| `1`   | `USER_ERROR`            | The user's input was invalid: bad flags, malformed `.ready-set.toml`, conflicting options, etc. Reported with a clear human-readable explanation.                    |
| `2`   | `SYSTEM_ERROR`          | An I/O, permission, or environmental error prevented completion. Not the user's fault.                                                                               |
| `3`   | `DEPENDENCY_MISSING`    | A required external tool (e.g. `git`, `cargo`, a plugin) was not found on PATH. The error message identifies the missing tool and how to install it.                 |
| `4`   | `PROJECT_REQUIREMENT_MISSING` | A lifecycle provider command was invoked without a plugin-declared project requirement, such as a Rust provider setup or workflow outside a Cargo workspace.      |
| `5`   | `CONTRACT_VIOLATION`    | A plugin violated the dispatcher↔plugin contract: `__describe` exceeded the 100 ms timeout, returned malformed JSON, etc. Reported by the dispatcher about a plugin. |
| `127` | `UNKNOWN_SUBCOMMAND`    | The dispatcher could not resolve the requested subcommand to a built-in or a `ready-set-<name>` binary on PATH. Mirrors the POSIX shell convention.                  |
| `128+N` | `SIGNALED(N)`         | A child process the dispatcher spawned was terminated by signal `N` (Unix `ExitStatus::code() == None`, with `ExitStatusExt::signal() == Some(N)`). The dispatcher emits exit code `128 + N` following the POSIX shell convention (e.g. `130` for SIGINT, `143` for SIGTERM). Reported by the dispatcher about a plugin; plugins MUST NOT return these codes from `main`. |

## Reserved ranges

| Range     | Use                                                                                                              |
|-----------|------------------------------------------------------------------------------------------------------------------|
| `0`       | Reserved: success.                                                                                               |
| `1–9`     | Reserved for the codes above. Future minor versions may add codes here.                                          |
| `64–78`   | Reserved for `sysexits.h` codes, used by some plugins for parity with traditional Unix utilities.                |
| `127`     | Reserved: unknown subcommand.                                                                                    |
| `128+N`   | Reserved: dispatcher-reported `SIGNALED(N)` for a child process killed by signal `N`. Also reserved by the OS. Plugins MUST NOT return these codes from `main`. |
| Other     | Plugins MAY use exit codes outside the reserved ranges for plugin-specific failure modes, but SHOULD prefer mapping such failures into one of the codes above. |

## Plugin obligations

Plugins SHOULD use the codes in the table above whenever the situation fits.
Plugins MAY define additional codes for plugin-specific failure modes, but
the additional codes MUST NOT collide with any reserved range.

The dispatcher does not transform plugin exit codes: when it `exec`s (Unix)
or spawns (Windows) a plugin, the plugin's exit code becomes the
dispatcher's exit code.

## SDK mirror

`ready_set_sdk::ExitCode` is the typed mirror of this table. Adding a
variant to the enum is a non-breaking change to the SDK iff the
corresponding numeric code already exists in this contract; otherwise it is
a contract change.
