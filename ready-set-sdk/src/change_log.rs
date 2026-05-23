//! Change-log records for reversibility.
//!
//! See
//! [`docs/contracts/change-log.md`](https://github.com/pulsearc-ai/ready-set/blob/main/docs/contracts/change-log.md)
//! for the source of truth.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{Error, Result};
use crate::fs as sdk_fs;

/// Operation recorded in a change log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeOp {
    /// File was created.
    Create,
    /// File was modified.
    Modify,
    /// File was deleted.
    Delete,
}

/// One change-log record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// Operation type.
    pub op: ChangeOp,
    /// Project-relative, forward-slash path.
    pub path: PathBuf,
    /// SHA-256 of the file before the change. `None` for `create`.
    pub before_sha256: Option<String>,
    /// SHA-256 of the file after the change. `None` for `delete`.
    pub after_sha256: Option<String>,
    /// RFC3339 UTC timestamp.
    #[serde(with = "rfc3339_utc")]
    pub ts: OffsetDateTime,
}

/// Append-only writer for one plugin invocation's records.
pub struct ChangeLog {
    project_root: PathBuf,
    plugin: String,
    file_path: PathBuf,
    writer: BufWriter<File>,
}

impl ChangeLog {
    /// Open (creating) the JSONL file for this plugin invocation.
    ///
    /// File path: `<project_root>/.ready-set/changes/<plugin>-<rfc3339>-<rand4>.jsonl`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] if the directory cannot be created or the file
    /// cannot be opened, or [`Error::Other`] if randomness collection fails.
    pub fn open(project_root: &Path, plugin: &str) -> Result<Self> {
        let dir = project_root.join(".ready-set/changes");
        fs::create_dir_all(&dir)?;

        let now = OffsetDateTime::now_utc();
        let stamp = filename_stamp(now)?;
        let rand = rand_suffix()?;
        let file_path = dir.join(format!("{plugin}-{stamp}-{rand}.jsonl"));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        Ok(Self {
            project_root: project_root.to_path_buf(),
            plugin: plugin.to_string(),
            file_path,
            writer: BufWriter::new(file),
        })
    }

    /// Path to the JSONL file this `ChangeLog` writes to.
    #[must_use]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Plugin name associated with this log.
    #[must_use]
    pub fn plugin(&self) -> &str {
        &self.plugin
    }

    /// Project root associated with this log.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Append a record. Each record is fsynced before the call returns.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on a write or fsync failure or
    /// [`Error::JsonParse`] if the record cannot be serialized.
    pub fn record(&mut self, record: &ChangeRecord) -> Result<()> {
        let line = serde_json::to_string(record)?;
        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }

    /// Flush buffered records.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Io`] on a write or fsync failure.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;
        Ok(())
    }
}

/// Read every record from `<project_root>/.ready-set/changes/`, sorted in
/// **reverse chronological order**.
///
/// Returns `(file_path, record)` tuples so callers can later remove
/// successfully reversed entries from their source file.
///
/// # Errors
///
/// Returns [`Error::Io`] if the changes directory cannot be read.
/// Malformed JSONL lines are skipped silently to keep `undo` resilient.
pub fn reverse_dir(project_root: &Path) -> Result<Vec<(PathBuf, ChangeRecord)>> {
    let dir = project_root.join(".ready-set/changes");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut all: Vec<(PathBuf, ChangeRecord)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { continue };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(record) = serde_json::from_str::<ChangeRecord>(&line) else {
                continue;
            };
            all.push((path.clone(), record));
        }
    }
    all.sort_by_key(|entry| std::cmp::Reverse(entry.1.ts));
    Ok(all)
}

/// Copy `source` into `<project_root>/.ready-set/backups/<sha>` (content
/// addressed). Returns the SHA used as the backup filename.
///
/// # Errors
///
/// Returns [`Error::Io`] if the source cannot be hashed or the backup cannot
/// be written.
pub fn backup_file(project_root: &Path, source: &Path) -> Result<String> {
    let sha = sdk_fs::sha256_file(source)?;
    let backups = project_root.join(".ready-set/backups");
    fs::create_dir_all(&backups)?;
    let dest = backups.join(&sha);
    if !dest.exists() {
        let bytes = fs::read(source)?;
        sdk_fs::atomic_write(&dest, &bytes)?;
    }
    Ok(sha)
}

fn filename_stamp(ts: OffsetDateTime) -> Result<String> {
    let formatted = ts
        .format(&Rfc3339)
        .map_err(|e| Error::Other(format!("rfc3339 format: {e}")))?;
    // Replace `:` with `-` for cross-platform filename safety, then trim
    // any sub-second precision so filenames stay compact.
    let without_subsec = match formatted.split_once('.') {
        Some((head, tail)) => {
            // tail looks like "123456789Z" — keep just the `Z` suffix.
            let z = if tail.contains('Z') { "Z" } else { "" };
            format!("{head}{z}")
        },
        None => formatted,
    };
    Ok(without_subsec.replace(':', "-"))
}

fn rand_suffix() -> Result<String> {
    let mut buf = [0_u8; 2];
    getrandom::fill(&mut buf).map_err(|e| Error::Other(format!("getrandom: {e}")))?;
    Ok(crate::fs::encode_hex_lower(&buf))
}

mod rfc3339_utc {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    pub fn serialize<S: Serializer>(ts: &OffsetDateTime, ser: S) -> Result<S::Ok, S::Error> {
        ts.format(&Rfc3339)
            .map_err(serde::ser::Error::custom)?
            .serialize(ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<OffsetDateTime, D::Error> {
        let raw = String::deserialize(de)?;
        OffsetDateTime::parse(&raw, &Rfc3339).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs as stdfs;

    #[test]
    fn writes_and_reads_records_in_reverse_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let mut log = ChangeLog::open(root, "go").unwrap();
        let earlier = ChangeRecord {
            op: ChangeOp::Create,
            path: PathBuf::from("a.txt"),
            before_sha256: None,
            after_sha256: Some("a".repeat(64)),
            ts: OffsetDateTime::from_unix_timestamp(1_000).unwrap(),
        };
        let later = ChangeRecord {
            op: ChangeOp::Modify,
            path: PathBuf::from("b.txt"),
            before_sha256: Some("b".repeat(64)),
            after_sha256: Some("c".repeat(64)),
            ts: OffsetDateTime::from_unix_timestamp(2_000).unwrap(),
        };
        log.record(&earlier).unwrap();
        log.record(&later).unwrap();
        drop(log);

        let all = reverse_dir(root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].1.path, PathBuf::from("b.txt"));
        assert_eq!(all[1].1.path, PathBuf::from("a.txt"));
    }

    #[test]
    fn backup_is_content_addressed_and_deduped() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let src = root.join("src.txt");
        stdfs::write(&src, b"hello").unwrap();
        let sha1 = backup_file(root, &src).unwrap();
        let sha2 = backup_file(root, &src).unwrap();
        assert_eq!(sha1, sha2);

        let backup = root.join(".ready-set/backups").join(&sha1);
        assert!(backup.exists());
    }

    #[test]
    fn empty_changes_dir_returns_empty_vec() {
        let dir = tempfile::tempdir().unwrap();
        let all = reverse_dir(dir.path()).unwrap();
        assert!(all.is_empty());
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let changes = dir.path().join(".ready-set/changes");
        stdfs::create_dir_all(&changes).unwrap();
        let path = changes.join("bad-2026-01-01T00-00-00Z-aaaa.jsonl");
        stdfs::write(&path, b"this is not json\n{also not}\n").unwrap();
        let all = reverse_dir(dir.path()).unwrap();
        assert!(all.is_empty());
    }
}
