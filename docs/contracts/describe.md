# Contract: `__describe` subcommand

| Field     | Value                                                                          |
|-----------|--------------------------------------------------------------------------------|
| Stability | `stable`                                                                       |
| Version   | 2                                                                              |
| Invoked   | `<plugin-binary> __describe`                                                   |
| Schema    | [`schemas/describe.schema.json`](schemas/describe.schema.json) (Draft 2020-12) |

The `__describe` subcommand is the fallback the dispatcher uses when a plugin
does not ship a [manifest sidecar](manifest.md). Plugins SHOULD ship a sidecar
when possible because it is faster and side-effect free. Plugins distributed
via `cargo install` typically must implement `__describe` because `cargo
install` does not place sidecar files.

## Plugin obligations

When invoked as `<binary> __describe`, the plugin MUST:

1. Print exactly **one line** of UTF-8 JSON to **stdout**, terminated by a
   single `\n`. The JSON object MUST conform to
   [`schemas/describe.schema.json`](schemas/describe.schema.json) (the same
   shape as the manifest sidecar).
2. Exit with code `0`.
3. Complete in **at most 100 ms** of wall-clock time.
4. Perform **no filesystem writes**.
5. Make **no network requests**.
6. Read **no environment variables** other than `READY_SET_*` (and even those
   only if it needs them; ideally `__describe` is constant).
7. Accept **no other arguments**. `<binary> __describe foo` SHOULD exit with
   a non-zero code.

The dispatcher enforces the 100 ms timeout. A plugin that exceeds it is
listed with "metadata unavailable".

## Argument-zero detection

A plugin MUST detect `__describe` **before** its main argument parser runs.
Otherwise a strict argument parser (e.g. clap with `deny_unknown_arguments`)
will reject the unknown subcommand and emit non-conforming output.

The Rust SDK provides `Describe::handle_arg0_describe(args)` to handle this
correctly. Non-Rust plugins should check `argv[1] == "__describe"` first
thing in `main`.

## Output shape

The JSON object has the same fields as the [manifest sidecar](manifest.md),
including the required `capabilities` array:

```json
{"description":"one-line summary","version":"1.2.3","stability":"stable","min_dispatcher_version":"0.1.0","platforms":["linux","macos","windows"],"capabilities":[]}
```

The required keys are those marked required in [manifest.md](manifest.md):
`description`, `version`, `stability`, `min_dispatcher_version`, `platforms`,
and `capabilities`. The optional `project_requirements` and `command_aliases`
arrays are omitted when empty. The output MUST fit on a single line (no embedded
newlines).

## Worked example

A minimal Rust implementation without the SDK:

```rust
fn main() {
    let mut args = std::env::args();
    let _exe = args.next();
    if args.next().as_deref() == Some("__describe") {
        println!(r#"{{"description":"Reference plugin","version":"0.1.0","stability":"stable","min_dispatcher_version":"0.1.0","platforms":["linux","macos","windows"],"capabilities":[]}}"#);
        return;
    }
    // ... normal plugin behavior
}
```

A POSIX shell plugin:

```sh
#!/bin/sh
if [ "$1" = "__describe" ]; then
  printf '{"description":"shell example","version":"0.0.1","stability":"experimental","min_dispatcher_version":"0.1.0","platforms":["linux","macos"],"capabilities":[]}\n'
  exit 0
fi
# ... normal plugin behavior
```
