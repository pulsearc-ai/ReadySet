# Contract: Change log JSONL

| Field       | Value                                                                                  |
|-------------|----------------------------------------------------------------------------------------|
| Stability   | `stable`                                                                               |
| Version     | 1                                                                                      |
| File path   | `<project_root>/.ready-set/changes/<plugin>-<rfc3339>-<rand4>.jsonl`                   |
| Backups     | `<project_root>/.ready-set/backups/<sha256>` (content-addressed)                       |
| Schema      | [`schemas/change-record.schema.json`](schemas/change-record.schema.json) (Draft 2020-12) |
| Format      | One JSON object per line, UTF-8, `\n` line terminator                                  |

Plugins that mutate the filesystem MUST record their writes to a JSONL change
log. The planned `ready-set undo` command will read the log to reverse changes
regardless of which plugin produced them. Plugins that do not use
`ready-set-sdk` are encouraged to follow the same on-disk format so
cross-plugin reversal works.

## File naming

A change log file is created per plugin per invocation. The filename has
three parts separated by `-`:

```
<plugin>-<rfc3339-timestamp>-<rand4>.jsonl
```

| Component        | Format                                                                          |
|------------------|---------------------------------------------------------------------------------|
| `<plugin>`       | The plugin name as it appears in `ready-set <plugin>` (no `ready-set-` prefix). |
| `<rfc3339>`      | RFC3339 UTC timestamp with `Z` suffix and second precision; `:` replaced with `-` for filename portability (`2025-05-10T15-04-05Z`). |
| `<rand4>`        | Four lowercase hex characters (`a-f0-9`) generated from a CSPRNG to avoid same-second collisions. |

Example: `go-2026-05-10T15-04-05Z-7a3f.jsonl`

## Record format

Each line is a single UTF-8 JSON object:

```json
{"op":"create","path":"rust-toolchain.toml","before_sha256":null,"after_sha256":"3b5d...","ts":"2026-05-10T15:04:05Z"}
```

| Field           | Type             | Required | Description                                                                                          |
|-----------------|------------------|----------|------------------------------------------------------------------------------------------------------|
| `op`            | string           | yes      | One of `create`, `modify`, `delete`.                                                                 |
| `path`          | string           | yes      | Relative to project root, normalized to forward-slash separators.                                    |
| `before_sha256` | string \| null   | yes      | Lowercase-hex SHA-256 of file contents prior to the change. `null` for `create` operations.          |
| `after_sha256`  | string \| null   | yes      | Lowercase-hex SHA-256 of file contents after the change. `null` for `delete` operations.             |
| `ts`            | string (RFC3339) | yes      | UTC timestamp with `Z` suffix when the record was written. May be later than the file timestamp due to buffering. |

### Operation semantics

| `op`     | `before_sha256` | `after_sha256` | Required backup                                                                                      |
|----------|-----------------|----------------|------------------------------------------------------------------------------------------------------|
| `create` | `null`          | hex            | None.                                                                                                |
| `modify` | hex             | hex            | A copy of the pre-modification content MUST exist at `<project_root>/.ready-set/backups/<before_sha256>`. |
| `delete` | hex             | `null`         | A copy of the deleted content MUST exist at `<project_root>/.ready-set/backups/<before_sha256>`.     |

### Path normalization

- Paths are relative to `<project_root>`.
- Paths use forward slashes on every platform (no backslashes).
- Paths MUST NOT contain `..` segments.
- Paths MUST NOT be empty.

## Backup storage

Pre-modification content is stored at:

```
<project_root>/.ready-set/backups/<before_sha256>
```

Content addressing means identical content shares storage and survives
renames cleanly. Backups are not garbage-collected automatically in v0.1.0.

## Atomicity

Plugins SHOULD write change records **after** the corresponding filesystem
write succeeds, in the order operations were applied. The SDK's `ChangeLog`
type fsyncs each record so a crash mid-write loses at most the trailing
record.

The planned `ready-set undo` command reverses records in **reverse
chronological order across all files in `.ready-set/changes/`**, not within a
single file. This means a plugin that performs A → B → C is undone as C → B →
A, and a later invocation D is undone before any of A/B/C.

## Reversal semantics

The planned `ready-set undo` command:

1. Enumerates all `<plugin>-*.jsonl` files in `.ready-set/changes/`.
2. Sorts records globally by `ts` (and within a single timestamp, by file
   then by line number).
3. For each record in **reverse** order:
   - Compute SHA-256 of the file currently at `path`.
   - If the current SHA does not match `after_sha256`, skip with a warning
     unless `--force` is set. The user has edited the file post-mutation
     and reversal could lose work.
   - For `create`: delete the file.
   - For `modify`: restore from `<project_root>/.ready-set/backups/<before_sha256>`.
   - For `delete`: restore from `<project_root>/.ready-set/backups/<before_sha256>`.
4. Successfully reversed records are removed from their JSONL file.
   When all records in a file are reversed, the file is deleted.
5. Records that cannot be reversed (mismatched SHA without `--force`,
   missing backup, etc.) are left in place with an explanation in the
   output.

## Worked example

A Rust provider setup invocation that creates `clippy.toml` and modifies the
root `Cargo.toml` produces `rust-2026-05-10T15-04-05Z-7a3f.jsonl`:

```jsonl
{"op":"create","path":"clippy.toml","before_sha256":null,"after_sha256":"3b5d4f...","ts":"2026-05-10T15:04:05Z"}
{"op":"modify","path":"Cargo.toml","before_sha256":"a92c1e...","after_sha256":"d44e07...","ts":"2026-05-10T15:04:05Z"}
```

A backup of the original `Cargo.toml` exists at
`.ready-set/backups/a92c1e...`. Once implemented, running `ready-set undo`
will delete `clippy.toml` and restore `Cargo.toml` from the backup.
