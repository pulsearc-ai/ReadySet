# Contract: Plugin metadata cache

| Field      | Value                                                            |
|------------|------------------------------------------------------------------|
| Stability  | `experimental-internal`                                          |
| Version    | 1                                                                |
| File path  | Platform-conventional cache dir (see below)                      |
| Schema     | [`schemas/cache.schema.json`](schemas/cache.schema.json) (Draft 2020-12) |

The dispatcher caches resolved plugin metadata to keep `ready-set --list`
fast. The file is **not part of the public ecosystem surface**: third-party
tools should not read or write it. It is versioned only to make future
schema rollouts clean.

## File location

| Platform | Path                                                |
|----------|-----------------------------------------------------|
| Linux    | `${XDG_CACHE_HOME:-$HOME/.cache}/ready-set/plugins.json` |
| macOS    | `~/Library/Caches/dev.ready-set/plugins.json`       |
| Windows  | `%LOCALAPPDATA%\ready-set\Cache\plugins.json`       |

Computed via the `directories` crate so the actual paths follow the OS
convention exactly.

## File shape

```json
{
  "schema_version": 1,
  "entries": {
    "<canonical_path>:<size_bytes>:<head4k_sha256>": {
      "manifest": {
        "description": "...",
        "version": "1.2.3",
        "stability": "stable",
        "min_dispatcher_version": "0.1.0",
        "platforms": ["linux", "macos", "windows"],
        "requires_cargo_workspace": false,
        "capabilities": []
      },
      "cached_at": "2026-05-10T15:04:05Z"
    }
  }
}
```

| Field            | Type    | Description                                                                                       |
|------------------|---------|---------------------------------------------------------------------------------------------------|
| `schema_version` | integer | Always `1` at v0.1.0. Mismatches invalidate the entire cache.                                     |
| `entries`        | object  | Map keyed by `<canonical_path>:<size_bytes>:<head4k_sha256>`. Values are cached manifest objects. |
| `entries[k].manifest`  | object        | Same shape as [`manifest.md`](manifest.md).                                              |
| `entries[k].cached_at` | string (RFC3339) | UTC timestamp when this entry was inserted. Entries older than 24 hours are ignored. |

### Cache key

The key encodes three parts joined by `:`:

| Part              | Source                                                                                          |
|-------------------|-------------------------------------------------------------------------------------------------|
| `canonical_path`  | Absolute, symlink-resolved path to the plugin binary, with platform-native separators.          |
| `size_bytes`      | Size of the binary in bytes as an unsigned decimal integer.                                     |
| `head4k_sha256`   | Lowercase-hex SHA-256 of the first 4096 bytes of the binary (or the entire binary if smaller).  |

Including the size and head hash makes most binary updates invalidate the
relevant cache entry without paying for a full content hash on every
`--list`.

## Cache miss behavior

When the cache is missing, corrupt, or has a `schema_version` the dispatcher
does not understand, the dispatcher MUST treat it as empty and rebuild it
from scratch. Corruption is never an error visible to the user.

## TTL

Cache entries are considered fresh for 24 hours (86400 seconds) regardless
of the cache key components. The TTL exists as a safety net against weird
edge cases (e.g. a plugin binary that legitimately has the same head hash
as a previous version after an update).

## Atomicity

Writes to `plugins.json` go through `ready_set_sdk::fs::atomic_write` (temp
file + fsync + rename). Concurrent writers may overwrite each other's
results; that is acceptable because the cache is reconstructible.
