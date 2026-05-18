//! Filesystem helpers used by both the dispatcher and plugins.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

/// Atomically write `bytes` to `path`.
///
/// Strategy: write to `<path>.<rand>.tmp` in the same directory, fsync the
/// file, then rename it onto the destination. The rename is atomic on every
/// supported platform (POSIX rename is atomic; Windows `MoveFileEx` with
/// `MOVEFILE_REPLACE_EXISTING` behaves likewise).
///
/// # Errors
///
/// Returns the underlying I/O error if any step fails. The temporary file is
/// removed before returning the error to avoid leaking droppings.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other(format!("path has no parent: {}", path.display())))?;
    fs::create_dir_all(parent)?;

    let tmp = tmp_path(path)?;

    let result = (|| -> Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        Ok(())
    })();

    if result.is_err() {
        drop(fs::remove_file(&tmp));
    }
    result
}

fn tmp_path(path: &Path) -> Result<std::path::PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Other(format!("path has no parent: {}", path.display())))?;
    let name = path
        .file_name()
        .ok_or_else(|| Error::Other(format!("path has no file name: {}", path.display())))?
        .to_os_string();

    let mut buf = [0_u8; 8];
    getrandom::fill(&mut buf).map_err(|e| Error::Other(format!("getrandom: {e}")))?;
    let mut tmp_name = std::ffi::OsString::new();
    tmp_name.push(&name);
    tmp_name.push(format!(".{}.tmp", encode_hex_lower(&buf)));
    Ok(parent.join(tmp_name))
}

/// Compute the SHA-256 of `path` as a lowercase hex string.
///
/// # Errors
///
/// Forwards I/O errors from opening or reading the file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(encode_hex_lower(&hasher.finalize()))
}

/// Compute the SHA-256 of `bytes` as a lowercase hex string.
#[must_use]
pub fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    encode_hex_lower(&hasher.finalize())
}

pub(crate) fn encode_hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Restrict `path` to the current user (mode `0600`) on Unix.
///
/// # Errors
///
/// Forwards I/O errors from `chmod`.
#[cfg(unix)]
pub fn restrict_to_user(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)?;
    Ok(())
}

/// Restrict `path` to the current user. No-op on Windows in v0.1.0.
///
/// # Errors
///
/// Always returns `Ok` on Windows; reserved for future ACL support.
#[cfg(windows)]
pub fn restrict_to_user(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("nested/dir/output.txt");
        atomic_write(&target, b"hello").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"hello");
    }

    #[test]
    fn atomic_write_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("output.txt");
        atomic_write(&target, b"v1").unwrap();
        atomic_write(&target, b"v2").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"v2");
    }

    #[test]
    fn sha256_matches_known_vector() {
        // sha256("abc") == ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
