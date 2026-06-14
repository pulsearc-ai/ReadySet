//! Plugin metadata cache.
//!
//! See `docs/contracts/cache.md`. Keyed by `(canonical_path, size_bytes,
//! head4k_sha256)`. TTL 86400s.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use ready_set_sdk::fs as sdk_fs;
use ready_set_sdk::manifest::Manifest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

const SCHEMA_VERSION: u32 = 1;
const TTL_SECONDS: i64 = 86_400;
const HEAD_BYTES: usize = 4096;

/// On-disk shape of the cache file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCache {
    /// Always `1` at v0.1.0.
    pub schema_version: u32,
    /// Cache entries keyed by `<canonical_path>:<size>:<head4k_sha256>`.
    pub entries: BTreeMap<String, CacheEntry>,
}

/// Cached manifest for one plugin binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Cached manifest payload.
    pub manifest: Manifest,
    /// RFC3339 UTC timestamp when this entry was inserted.
    pub cached_at: String,
}

impl Default for PluginCache {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

impl PluginCache {
    /// Resolve the cache file path for the current platform.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        ProjectDirs::from("dev", "ready-set", "ready-set")
            .map(|d| d.cache_dir().join("plugins.json"))
    }

    /// Load the cache from `path`. Missing or corrupt files yield an empty
    /// cache rather than an error.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(raw) = std::fs::read(path) else {
            return Self::default();
        };
        let Ok(parsed) = serde_json::from_slice::<Self>(&raw) else {
            return Self::default();
        };
        if parsed.schema_version != SCHEMA_VERSION {
            return Self::default();
        }
        parsed
    }

    /// Save the cache atomically.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O failure from the atomic-write helper.
    pub fn save(&self, path: &Path) -> ready_set_sdk::Result<()> {
        let bytes = serde_json::to_vec(self)?;
        sdk_fs::atomic_write(path, &bytes)
    }

    /// Look up a cached manifest. Returns `None` if missing or expired.
    #[must_use]
    pub fn get(&self, key: &CacheKey) -> Option<&Manifest> {
        let entry = self.entries.get(&key.encode())?;
        let cached = OffsetDateTime::parse(&entry.cached_at, &Rfc3339).ok()?;
        let age = (OffsetDateTime::now_utc() - cached).whole_seconds();
        if (0..TTL_SECONDS).contains(&age) {
            Some(&entry.manifest)
        } else {
            None
        }
    }

    /// Insert a cache entry, replacing any prior entry with the same key.
    ///
    /// # Errors
    ///
    /// Returns [`ready_set_sdk::Error::Other`] if the timestamp cannot be
    /// formatted.
    pub fn insert(&mut self, key: &CacheKey, manifest: Manifest) -> ready_set_sdk::Result<()> {
        let cached_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|e| ready_set_sdk::Error::other(format!("rfc3339 format: {e}")))?;
        self.entries.insert(
            key.encode(),
            CacheEntry {
                manifest,
                cached_at,
            },
        );
        Ok(())
    }
}

/// Composite cache key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheKey {
    /// Canonical, symlink-resolved binary path.
    pub canonical_path: PathBuf,
    /// Size of the binary in bytes.
    pub size_bytes: u64,
    /// SHA-256 of the first 4096 bytes (or the whole file if shorter).
    pub head4k_sha256: String,
}

impl CacheKey {
    /// Construct a cache key by reading metadata + the head of the binary.
    ///
    /// # Errors
    ///
    /// Forwards I/O errors from canonicalizing the path or reading its head.
    pub fn for_binary(path: &Path) -> ready_set_sdk::Result<Self> {
        let canonical_path = std::fs::canonicalize(path)?;
        let metadata = std::fs::metadata(&canonical_path)?;
        let size_bytes = metadata.len();

        let mut f = File::open(&canonical_path)?;
        let mut buf = vec![0_u8; HEAD_BYTES];
        let read = f.read(&mut buf)?;
        let mut hasher = Sha256::new();
        hasher.update(&buf[..read]);
        let head4k_sha256 = encode_hex_lower(&hasher.finalize());

        Ok(Self {
            canonical_path,
            size_bytes,
            head4k_sha256,
        })
    }

    fn encode(&self) -> String {
        format!(
            "{}:{}:{}",
            self.canonical_path.display(),
            self.size_bytes,
            self.head4k_sha256
        )
    }
}

fn encode_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ready_set_sdk::describe::{Platform, Stability};

    fn fixture_manifest() -> Manifest {
        Manifest {
            description: "x".into(),
            version: "0.1.0".parse().unwrap(),
            stability: Stability::Stable,
            min_dispatcher_version: "0.1.0".parse().unwrap(),
            platforms: vec![Platform::Linux],
            project_requirements: Vec::new(),
            capabilities: Vec::new(),
            command_aliases: Vec::new(),
        }
    }

    #[test]
    fn schema_mismatch_yields_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.json");
        std::fs::write(&path, br#"{"schema_version":99,"entries":{}}"#).unwrap();
        let loaded = PluginCache::load(&path);
        assert_eq!(loaded.schema_version, SCHEMA_VERSION);
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn corrupt_file_yields_empty_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.json");
        std::fs::write(&path, b"not json").unwrap();
        let loaded = PluginCache::load(&path);
        assert!(loaded.entries.is_empty());
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plugins.json");
        let bin = dir.path().join("ready-set-foo");
        std::fs::write(&bin, b"#!/bin/sh\necho hi\n").unwrap();
        let key = CacheKey::for_binary(&bin).unwrap();
        let mut cache = PluginCache::default();
        cache.insert(&key, fixture_manifest()).unwrap();
        cache.save(&path).unwrap();
        let loaded = PluginCache::load(&path);
        assert!(loaded.get(&key).is_some());
    }
}
