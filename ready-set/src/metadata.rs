//! Plugin metadata resolution.
//!
//! Resolution order:
//! 1. Cache hit → use it.
//! 2. Sidecar manifest TOML next to the binary → parse it, cache it.
//! 3. Spawn `<binary> __describe` with a 100 ms wall-clock timeout, parse
//!    one line of JSON, cache it.
//! 4. None of the above → return `None`. The dispatcher still lists the
//!    plugin under "metadata unavailable".

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ready_set_sdk::manifest::Manifest;

use crate::cache::{CacheKey, PluginCache};
use crate::discovery::PluginEntry;

const DESCRIBE_TIMEOUT: Duration = Duration::from_millis(100);
const POLL_INTERVAL: Duration = Duration::from_millis(2);

/// Resolve metadata for `entry` using cache → sidecar → `__describe`.
///
/// Returns `None` if every method failed.
#[must_use]
pub fn resolve_metadata(entry: &PluginEntry, cache: &mut PluginCache) -> Option<Manifest> {
    if let Ok(key) = CacheKey::for_binary(&entry.binary_path) {
        if let Some(cached) = cache.get(&key) {
            return Some(cached.clone());
        }

        if let Some(m) = entry.manifest.clone() {
            drop(cache.insert(&key, m.clone()));
            return Some(m);
        }

        if let Some(m) = invoke_describe(entry) {
            drop(cache.insert(&key, m.clone()));
            return Some(m);
        }
    } else if let Some(m) = entry.manifest.clone() {
        return Some(m);
    }
    None
}

fn invoke_describe(entry: &PluginEntry) -> Option<Manifest> {
    let mut child = Command::new(&entry.binary_path)
        .arg("__describe")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                break;
            },
            Ok(None) => {},
            Err(_) => return None,
        }
        if start.elapsed() >= DESCRIBE_TIMEOUT {
            drop(child.kill());
            drop(child.wait());
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    let mut stdout = child.stdout.take()?;
    let mut buf = String::new();
    stdout.read_to_string(&mut buf).ok()?;
    let line = buf.lines().next()?.trim();
    let raw_value = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let manifest: Manifest = serde_json::from_value(raw_value).ok()?;
    Some(manifest)
}
