//! Detect environment variables used by a project.
//!
//! Two sources contribute to the inventory:
//! - `.env` and `.env.example` files at the project root, parsed for `KEY=...`
//!   declarations.
//! - Source files (Rust + TypeScript / JavaScript family) scanned with regular
//!   expressions for `std::env::var("X")`, `env!("X")`, `process.env.X`, and
//!   `import.meta.env.X` references.
//!
//! The scan is heuristic: dynamic lookups like
//! `env::var(format!("X_{i}", ...))` are not detected. Pin such names in
//! `.env.example` manually.

use std::collections::BTreeSet;
use std::path::Path;

use regex::Regex;
use walkdir::WalkDir;

use crate::config::SecretsConfig;
use crate::scaffold::strip_managed_block;

/// One snapshot of what the project declares vs. what its source references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// Names declared in `.env.example` at the project root.
    pub declared: BTreeSet<String>,
    /// Names declared in `.env` at the project root (treated as advisory —
    /// these are local secrets, not a contract).
    pub local: BTreeSet<String>,
    /// Names referenced by source files inside the project tree.
    pub referenced: BTreeSet<String>,
    /// `.env.example` existed when this inventory was taken.
    pub env_example_present: bool,
    /// Declared-but-unreferenced names are intentional for this project.
    pub allow_declared_orphans: bool,
}

impl Inventory {
    /// Names referenced in code but missing from `.env.example`.
    #[must_use]
    pub fn missing_from_example(&self) -> Vec<String> {
        self.referenced
            .difference(&self.declared)
            .cloned()
            .collect()
    }

    /// Names declared in `.env.example` but not referenced anywhere in code.
    #[must_use]
    pub fn orphans_in_example(&self) -> Vec<String> {
        if self.allow_declared_orphans {
            return Vec::new();
        }
        self.declared
            .difference(&self.referenced)
            .cloned()
            .collect()
    }

    /// Union of every name we know about. Used as the seed for `.env.example`
    /// rendering.
    #[must_use]
    pub fn all_names(&self) -> BTreeSet<String> {
        let mut out = self.declared.clone();
        out.extend(self.local.iter().cloned());
        out.extend(self.referenced.iter().cloned());
        out
    }
}

/// Take an inventory snapshot of `root`.
///
/// # Errors
///
/// Forwards filesystem errors from reading `.env*` files or walking the tree.
pub fn scan(root: &Path) -> std::io::Result<Inventory> {
    let config = SecretsConfig::load(root)?;
    let env_example_path = root.join(".env.example");
    let env_example_present = env_example_path.is_file();
    let ignored = config.ignored_names();
    let declared =
        filter_ignored_names(read_env_keys_many(&config.declared_files(root))?, &ignored);
    let local = filter_ignored_names(
        read_local_env_keys_many(&config.local_files(root))?,
        &ignored,
    );
    let referenced = scan_source_tree(root, &config);
    Ok(Inventory {
        declared,
        local,
        referenced,
        env_example_present,
        allow_declared_orphans: config.inventory.allow_declared_orphans,
    })
}

fn read_env_keys_many(paths: &[std::path::PathBuf]) -> std::io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for path in paths {
        out.extend(read_env_keys(path, true)?);
    }
    Ok(out)
}

fn read_local_env_keys_many(paths: &[std::path::PathBuf]) -> std::io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for path in paths {
        out.extend(read_env_keys(path, false)?);
    }
    Ok(out)
}

fn filter_ignored_names(
    mut names: BTreeSet<String>,
    ignored: &BTreeSet<String>,
) -> BTreeSet<String> {
    names.retain(|name| !is_ignored_env_name(name) && !ignored.contains(name));
    names
}

fn read_env_keys(path: &Path, strip_ready_set_block: bool) -> std::io::Result<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(err) => return Err(err),
    };
    let content = if strip_ready_set_block {
        strip_managed_block(&raw)
    } else {
        raw
    };
    for line in content.lines() {
        if let Some(key) = parse_env_key(line) {
            out.insert(key);
        }
    }
    Ok(out)
}

fn parse_env_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let rest = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    let (key, _value) = rest.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !is_valid_env_name(key) {
        return None;
    }
    Some(key.to_owned())
}

fn is_valid_env_name(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn scan_source_tree(root: &Path, config: &SecretsConfig) -> BTreeSet<String> {
    let rust_re = Regex::new(
        r#"(?x)
        (?:
          \b env \s* :: \s* var (?:_os)? \s* \( \s* " ([A-Z_][A-Z0-9_]*) "
          |
          \b env! \s* \( \s* " ([A-Z_][A-Z0-9_]*) "
          |
          \b option_env! \s* \( \s* " ([A-Z_][A-Z0-9_]*) "
        )
        "#,
    )
    .expect("rust env regex");
    let ts_re = Regex::new(
        r#"(?x)
        (?:
          \b process \. env \. ([A-Z_][A-Z0-9_]*) \b
          |
          \b process \. env \[ \s* (?:'|") ([A-Z_][A-Z0-9_]*) (?:'|") \s* \]
          |
          \b import \. meta \. env \. ([A-Z_][A-Z0-9_]*) \b
          |
          \b import \. meta \. env \[ \s* (?:'|") ([A-Z_][A-Z0-9_]*) (?:'|") \s* \]
        )
        "#,
    )
    .expect("ts env regex");

    let shell_first_non_empty_re =
        Regex::new(r"\bfirst_non_empty\s+([A-Z][A-Z0-9_]*(?:\s+[A-Z][A-Z0-9_]*)*)")
            .expect("shell first_non_empty regex");

    let ignored = config.ignored_names();
    let mut out: BTreeSet<String> = BTreeSet::new();
    for source_root in config.source_roots(root) {
        if !source_root.exists() {
            continue;
        }
        let walker = WalkDir::new(&source_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !is_excluded_dir(entry.path(), root));
        for entry in walker.flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(kind) = source_kind(path) else {
                continue;
            };
            let Ok(content) = std::fs::read_to_string(path) else {
                continue;
            };
            match kind {
                SourceKind::Rust => {
                    let stripped = strip_test_modules(&content);
                    collect_matches(&rust_re, &stripped, &ignored, &mut out);
                },
                SourceKind::Web => collect_matches(&ts_re, &content, &ignored, &mut out),
                SourceKind::Shell => {
                    collect_matches(&ts_re, &content, &ignored, &mut out);
                    collect_shell_first_non_empty(
                        &shell_first_non_empty_re,
                        &content,
                        &ignored,
                        &mut out,
                    );
                },
            }
        }
    }
    out
}

fn collect_matches(
    re: &Regex,
    content: &str,
    ignored: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    for caps in re.captures_iter(content) {
        for i in 1..caps.len() {
            if let Some(m) = caps.get(i) {
                let name = m.as_str();
                if !name.is_empty() && !is_ignored_env_name(name) && !ignored.contains(name) {
                    out.insert(name.to_owned());
                }
            }
        }
    }
}

fn collect_shell_first_non_empty(
    re: &Regex,
    content: &str,
    ignored: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    for caps in re.captures_iter(content) {
        let Some(names) = caps.get(1) else {
            continue;
        };
        for name in names.as_str().split_whitespace() {
            if !is_ignored_env_name(name) && !ignored.contains(name) {
                out.insert(name.to_owned());
            }
        }
    }
}

/// True for env-var names that should never appear in managed secrets files.
///
/// This covers OS-set vars, cargo build vars, common framework-injected vars,
/// and ready-set's own SDK contract vars.
#[must_use]
pub fn is_ignored_env_name(name: &str) -> bool {
    // ready-set's own dispatcher contract — referenced by plugins via the
    // SDK's Context::from_env(), never by application code.
    if name.starts_with("READY_SET_") {
        return true;
    }
    matches!(
        name,
        // POSIX / shell
        "HOME"
            | "PATH"
            | "PATHEXT"
            | "USER"
            | "LOGNAME"
            | "PWD"
            | "OLDPWD"
            | "LANG"
            | "SHELL"
            | "TMPDIR"
            | "TEMP"
            | "TMP"
            | "TERM"
            // cargo build-time
            | "OUT_DIR"
            | "PROFILE"
            | "TARGET"
            | "HOST"
            | "OPT_LEVEL"
            | "MANIFEST_DIR"
            // common framework-injected JS / Vite / Next
            | "NODE_ENV"
            | "DEV"
            | "MODE"
            | "PROD"
            | "SSR"
            | "BASE_URL"
    )
}

#[derive(Debug, Clone, Copy)]
enum SourceKind {
    Rust,
    Web,
    Shell,
}

fn source_kind(path: &Path) -> Option<SourceKind> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(SourceKind::Rust),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Some(SourceKind::Web),
        "sh" => Some(SourceKind::Shell),
        _ => None,
    }
}

/// Remove test-gated blocks from Rust source before regex scanning.
///
/// This catches inline helper modules whose `env::var("FIXTURE")` calls would
/// otherwise leak into the inventory as false-positive secrets.
///
/// Strategy: find each cfg-test attribute, skip ahead to the first `{`, and
/// strip until the matching brace (string and char literals tracked so braces
/// inside don't trip the counter). Block comments and line comments are
/// honored too. Attributes without a following block (rare — e.g.
/// `#[cfg(test)] use ...;`) are left intact; the regex won't match a `use`
/// anyway.
#[must_use]
pub fn strip_test_modules(content: &str) -> String {
    const MARKERS: &[&[u8]] = &[
        b"#[cfg(test)]",
        b"#[cfg_attr(test,",
        b"#[cfg_attr(test ,",
        b"#[cfg(all(test",
        b"#[cfg(any(test",
    ];
    let bytes = content.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let matched_marker = MARKERS.iter().find(|m| bytes[i..].starts_with(m)).copied();
        let Some(marker) = matched_marker else {
            out.push(bytes[i]);
            i += 1;
            continue;
        };
        let attr_end = i + marker.len();
        let Some(brace_offset) = find_opening_brace_bytes(&bytes[attr_end..]) else {
            out.push(bytes[i]);
            i += 1;
            continue;
        };
        let brace_start = attr_end + brace_offset;
        let Some(block_end) = find_matching_brace(bytes, brace_start) else {
            out.push(bytes[i]);
            i += 1;
            continue;
        };
        // Skip the cfg attribute + the entire block.
        i = block_end + 1;
    }
    // Safe: we only ever copy whole bytes from `content` (a valid UTF-8 str)
    // or skip whole bytes; we never split inside a multi-byte sequence. The
    // matcher only triggers on ASCII byte sequences so the boundaries stay
    // aligned with the original UTF-8 grapheme structure.
    String::from_utf8(out).unwrap_or_else(|e| {
        // Defensive fallback if the assumption above is ever wrong.
        String::from_utf8_lossy(e.as_bytes()).into_owned()
    })
}

fn find_opening_brace_bytes(rest: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            b'{' => return Some(i),
            b';' => return None,
            _ => i += 1,
        }
    }
    None
}

fn find_matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut i = open;
    while i < bytes.len() {
        let c = bytes[i];
        // Skip line comments.
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // Skip block comments (non-nested approximation; matches Rust spec
        // poorly for nested cases but the scanner is heuristic anyway).
        if c == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        // Skip string literals (track escapes).
        if c == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        // Skip char literals (b'x', '\n', etc.).
        if c == b'\'' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'\'' {
                    i += 1;
                    break;
                }
                i += 1;
                if i - open > 4 {
                    break; // not a char literal after all
                }
            }
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn is_excluded_dir(path: &Path, root: &Path) -> bool {
    if path == root {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".git"
            | ".next"
            | ".turbo"
            | ".vercel"
            | ".cache"
            | ".ready-set"
            | "coverage"
            | ".venv"
            | "venv"
            | "__pycache__"
            | "tests"
            | "benches"
            | "examples"
    )
}

/// Parse the user-curated portion of an existing `.env.example`, stripping the
/// managed block so [`scaffold`] can merge cleanly.
///
/// Returns `(prelude, user_keys)` — the prelude is the original content with
/// any managed block removed (preserving the user's comments and ordering), and
/// `user_keys` are the keys defined outside the managed block.
#[must_use]
pub fn split_env_example(raw: &str) -> (String, BTreeSet<String>) {
    let prelude = strip_managed_block(raw);
    let mut user_keys = BTreeSet::new();
    for line in prelude.lines() {
        if let Some(key) = parse_env_key(line) {
            user_keys.insert(key);
        }
    }
    (prelude, user_keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> SecretsConfig {
        SecretsConfig::default()
    }

    #[test]
    fn parses_simple_kv_lines() {
        assert_eq!(parse_env_key("FOO=bar").as_deref(), Some("FOO"));
        assert_eq!(parse_env_key("  EXPORTED=1").as_deref(), Some("EXPORTED"));
        assert_eq!(parse_env_key("export FOO=bar").as_deref(), Some("FOO"));
        assert_eq!(parse_env_key("# FOO=bar"), None);
        assert_eq!(parse_env_key(""), None);
        assert_eq!(parse_env_key("not-a-key"), None);
        assert_eq!(parse_env_key("123_BAD=1"), None);
    }

    #[test]
    fn extracts_rust_env_refs() {
        let rust = r#"
            let token = std::env::var("API_TOKEN").unwrap();
            let url = env::var_os("DATABASE_URL").unwrap();
            let pinned = env!("BUILD_HASH");
            let optional = option_env!("OPTIONAL_FLAG");
        "#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.rs"), rust).unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        for expected in ["API_TOKEN", "DATABASE_URL", "BUILD_HASH", "OPTIONAL_FLAG"] {
            assert!(found.contains(expected), "missing {expected}: {found:?}");
        }
    }

    #[test]
    fn extracts_ts_env_refs() {
        let ts = r#"
            const a = process.env.NEXT_PUBLIC_URL;
            const b = process.env["RESEND_KEY"];
            const c = import.meta.env.VITE_API_BASE;
            const d = import.meta.env['VITE_OTHER'];
        "#;
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.ts"), ts).unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        for expected in [
            "NEXT_PUBLIC_URL",
            "RESEND_KEY",
            "VITE_API_BASE",
            "VITE_OTHER",
        ] {
            assert!(found.contains(expected), "missing {expected}: {found:?}");
        }
    }

    #[test]
    fn skips_excluded_dirs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(
            dir.path().join("node_modules/pkg/index.js"),
            "process.env.LEAKED",
        )
        .unwrap();
        std::fs::write(dir.path().join("real.ts"), "process.env.REAL").unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("REAL"));
        assert!(!found.contains("LEAKED"));
    }

    #[test]
    fn read_env_keys_handles_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let out = read_env_keys(&dir.path().join("nope"), true).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn declared_env_keys_ignore_generated_managed_block() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "PINNED=\n# >>> ready-set-encrypt managed >>>\nSTALE_FROM_OLD_SCAN=\n# <<< ready-set-encrypt managed <<<\n",
        )
        .unwrap();

        let inv = scan(dir.path()).unwrap();

        assert!(inv.declared.contains("PINNED"));
        assert!(!inv.declared.contains("STALE_FROM_OLD_SCAN"));
    }

    #[test]
    fn configured_ignored_names_are_removed_from_declared_and_local_inventory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ready-set/plugins/secrets")).unwrap();
        std::fs::write(
            dir.path().join(".ready-set/plugins/secrets/config.toml"),
            r#"[inventory]
ignore_names = ["READYSET_INTERNAL_MARKER"]
"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".env.example"),
            "APP_SECRET=\nREADYSET_INTERNAL_MARKER=\nPATH=\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join(".env"),
            "APP_SECRET=local\nREADYSET_INTERNAL_MARKER=1\nPATH=/bin\n",
        )
        .unwrap();

        let inv = scan(dir.path()).unwrap();

        assert!(inv.declared.contains("APP_SECRET"));
        assert!(inv.local.contains("APP_SECRET"));
        assert!(!inv.declared.contains("READYSET_INTERNAL_MARKER"));
        assert!(!inv.local.contains("READYSET_INTERNAL_MARKER"));
        assert!(!inv.declared.contains("PATH"));
        assert!(!inv.local.contains("PATH"));
    }

    #[test]
    fn scan_classifies_inventory_states() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env.example"), "DECLARED=\nORPHAN=\n").unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"env::var("DECLARED");env::var("MISSING_FROM_EXAMPLE");"#,
        )
        .unwrap();
        let inv = scan(dir.path()).unwrap();
        assert!(inv.env_example_present);
        assert!(inv.declared.contains("DECLARED"));
        assert!(inv.declared.contains("ORPHAN"));
        assert!(inv.referenced.contains("DECLARED"));
        assert!(inv.referenced.contains("MISSING_FROM_EXAMPLE"));
        assert_eq!(inv.missing_from_example(), vec!["MISSING_FROM_EXAMPLE"]);
        assert_eq!(inv.orphans_in_example(), vec!["ORPHAN"]);
    }

    #[test]
    fn denylist_drops_system_env_vars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"
                let _ = std::env::var("HOME");
                let _ = std::env::var("PATH");
                let _ = std::env::var("TMPDIR");
                let _ = std::env::var("REAL_APP_KEY");
            "#,
        )
        .unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("REAL_APP_KEY"));
        assert!(!found.contains("HOME"));
        assert!(!found.contains("PATH"));
        assert!(!found.contains("TMPDIR"));
    }

    #[test]
    fn denylist_drops_ready_set_prefixed_vars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"
                let _ = std::env::var("READY_SET_PROJECT_ROOT");
                let _ = std::env::var("READY_SET_OUTPUT");
                let _ = std::env::var("READY_SET_FUTURE_VAR_THAT_DOES_NOT_EXIST_YET");
                let _ = std::env::var("READY_SET_API_KEY"); // edge case: should still skip
                let _ = std::env::var("APP_DATABASE_URL");
            "#,
        )
        .unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("APP_DATABASE_URL"));
        assert!(!found.iter().any(|n| n.starts_with("READY_SET_")));
    }

    #[test]
    fn denylist_drops_framework_injected_js_vars() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("app.ts"),
            "
                const env = process.env.NODE_ENV;
                const mode = import.meta.env.MODE;
                const dev = import.meta.env.DEV;
                const real = import.meta.env.VITE_API_BASE;
            ",
        )
        .unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("VITE_API_BASE"));
        assert!(!found.contains("NODE_ENV"));
        assert!(!found.contains("MODE"));
        assert!(!found.contains("DEV"));
    }

    #[test]
    fn cfg_test_module_is_stripped_from_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("main.rs"),
            r#"
                fn run() { let _ = std::env::var("PROD_KEY"); }

                #[cfg(test)]
                mod tests {
                    use super::*;

                    #[test]
                    fn fixture() {
                        let _ = std::env::var("TEST_ONLY_KEY");
                        let _ = std::env::var("ANOTHER_TEST_KEY");
                    }
                }
            "#,
        )
        .unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("PROD_KEY"));
        assert!(!found.contains("TEST_ONLY_KEY"));
        assert!(!found.contains("ANOTHER_TEST_KEY"));
    }

    #[test]
    fn cfg_attr_test_block_is_stripped() {
        let stripped = strip_test_modules(
            r#"
                fn alive() { let _ = env::var("KEEP"); }

                #[cfg_attr(test, derive(Debug))]
                #[cfg(test)]
                fn helper() {
                    let _ = env::var("DROP");
                }
            "#,
        );
        assert!(stripped.contains("KEEP"));
        assert!(!stripped.contains("DROP"));
    }

    #[test]
    fn strip_test_modules_handles_braces_inside_strings() {
        let stripped = strip_test_modules(
            r#"
                #[cfg(test)]
                mod tests {
                    let s = "}{}{}";
                    let _ = env::var("INSIDE");
                }
                fn after() { let _ = env::var("AFTER"); }
            "#,
        );
        assert!(stripped.contains("AFTER"));
        assert!(!stripped.contains("INSIDE"));
    }

    #[test]
    fn strip_test_modules_passthrough_when_no_cfg_test() {
        let src = r#"fn x() { let _ = env::var("Y"); }"#;
        assert_eq!(strip_test_modules(src), src);
    }

    #[test]
    fn tests_dir_is_excluded_from_walk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::create_dir_all(dir.path().join("tests")).unwrap();
        std::fs::write(
            dir.path().join("src/main.rs"),
            r#"fn run() { let _ = std::env::var("REAL"); }"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tests/e2e.rs"),
            r#"let _ = std::env::var("E2E_FIXTURE");"#,
        )
        .unwrap();
        let found = scan_source_tree(dir.path(), &default_config());
        assert!(found.contains("REAL"));
        assert!(!found.contains("E2E_FIXTURE"));
    }

    #[test]
    fn is_ignored_env_name_matrix() {
        assert!(is_ignored_env_name("HOME"));
        assert!(is_ignored_env_name("PATH"));
        assert!(is_ignored_env_name("READY_SET_PROJECT_ROOT"));
        assert!(is_ignored_env_name("NODE_ENV"));
        assert!(!is_ignored_env_name("DATABASE_URL"));
        assert!(!is_ignored_env_name("PORT")); // ambiguous; keep
        assert!(!is_ignored_env_name("SESSION_SECRET"));
    }
}
