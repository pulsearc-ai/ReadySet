//! Per-OS sandbox wrapping for spawned subprocesses.
//!
//! Every `exec`/`deploy` command in the rotation backend is wrapped via the
//! host OS's sandboxing primitive so a malicious or buggy provider CLI cannot
//! write outside the project root + tmp + caches + per-secret allowlist.
//!
//! - **macOS:** `sandbox-exec` with a `TinyScheme` profile (default deny + allow
//!   network + allow file-read + write-allowlist).
//! - **Linux:** `bubblewrap` (`bwrap`) with `--ro-bind /` as the base
//!   (everything outside the writable allowlist becomes read-only) plus
//!   per-path `--bind` overlays for `project_root`, tmp, cache, and extras.

use std::path::{Path, PathBuf};

use ready_set_sdk::{Error, Result};

/// Inputs to [`wrap`]. Computed per-invocation from the manifest + project
/// context.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Project root; all writes inside are allowed.
    pub project_root: PathBuf,
    /// Additional paths added to the writable allowlist. `~` is expanded
    /// against the current user's home dir.
    pub extra_write_paths: Vec<PathBuf>,
    /// When true, [`wrap`] is a no-op and returns `{ sandboxed: false }`. Used
    /// by the manifest's `unsandboxed = true` escape hatch.
    pub unsandboxed: bool,
}

/// Result of wrapping an argv. `argv` is what
/// `Command::new(argv[0]).args(&argv[1..])` should invoke.
#[derive(Debug, Clone)]
pub struct WrapResult {
    /// The (possibly wrapped) argv to spawn.
    pub argv: Vec<String>,
    /// Was the wrap actually applied? `false` only when `config.unsandboxed`.
    pub sandboxed: bool,
    /// Stable label for the audit log. `Some(...)` when sandboxed.
    pub platform_label: Option<&'static str>,
}

/// Wrap `argv` for sandboxed execution using the host OS backend.
///
/// # Errors
///
/// Returns [`Error::MissingDependency`] if the backend's required tool
/// (`sandbox-exec` on macOS, `bwrap` on Linux, `ready-set-encrypt-launcher`
/// on Windows) is not on PATH.
pub fn wrap(argv: Vec<String>, config: &SandboxConfig) -> Result<WrapResult> {
    if config.unsandboxed {
        return Ok(WrapResult {
            argv,
            sandboxed: false,
            platform_label: None,
        });
    }
    #[cfg(target_os = "macos")]
    {
        macos::wrap(argv, config)
    }
    #[cfg(target_os = "linux")]
    {
        linux::wrap(argv, config)
    }
    #[cfg(target_os = "windows")]
    {
        windows::wrap(argv, config)
    }
}

// ---------------------------------------------------------------------------
// macOS backend
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod macos {
    use super::{
        Error, PathBuf, Result, SandboxConfig, WrapResult, escape_path, expand_tilde, home_dir,
    };

    /// Stable label written to audit entries identifying the macOS backend.
    pub const PLATFORM_LABEL: &str = "macos-sandbox-exec";
    const PROFILE_TEMPLATE: &str = include_str!("templates/sandbox.sb");

    pub fn wrap(argv: Vec<String>, config: &SandboxConfig) -> Result<WrapResult> {
        if which::which("sandbox-exec").is_err() {
            return Err(Error::MissingDependency {
                name: "sandbox-exec".into(),
                hint: Some(
                    "sandbox-exec is required for ready-set-encrypt on macOS; \
                     set `unsandboxed = true` on the secret to bypass at your own risk"
                        .into(),
                ),
            });
        }
        let profile = render_profile(config);
        let mut wrapped: Vec<String> = Vec::with_capacity(argv.len() + 4);
        wrapped.push("sandbox-exec".into());
        wrapped.push("-p".into());
        wrapped.push(profile);
        wrapped.push("--".into());
        wrapped.extend(argv);
        Ok(WrapResult {
            argv: wrapped,
            sandboxed: true,
            platform_label: Some(PLATFORM_LABEL),
        })
    }

    /// Render the macOS sandbox-exec profile text for a given config. Pure;
    /// testable in isolation.
    #[must_use]
    pub fn render_profile(config: &SandboxConfig) -> String {
        let project_root = config.project_root.display().to_string();
        let tmpdir = std::env::temp_dir().display().to_string();
        let home = home_dir().display().to_string();
        let extra_writes = render_extra_writes(&config.extra_write_paths, &home);

        PROFILE_TEMPLATE
            .replace("<PROJECT_ROOT>", &escape_path(&project_root))
            .replace("<TMPDIR>", &escape_path(&tmpdir))
            .replace("<HOME>", &escape_path(&home))
            .replace("<EXTRA_WRITES>", &extra_writes)
    }

    fn render_extra_writes(paths: &[PathBuf], home: &str) -> String {
        if paths.is_empty() {
            return String::new();
        }
        paths
            .iter()
            .map(|p| {
                let expanded = expand_tilde(p, home);
                format!("  (subpath \"{}\")", escape_path(&expanded))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(all(test, target_os = "macos"))]
use macos::{PLATFORM_LABEL, render_profile};

// ---------------------------------------------------------------------------
// Linux backend
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use super::{Error, PathBuf, Result, SandboxConfig, WrapResult, expand_tilde, home_dir};

    /// Stable label written to audit entries identifying the Linux backend.
    pub const PLATFORM_LABEL: &str = "linux-bwrap";

    pub fn wrap(argv: Vec<String>, config: &SandboxConfig) -> Result<WrapResult> {
        if which::which("bwrap").is_err() {
            return Err(Error::MissingDependency {
                name: "bwrap".into(),
                hint: Some(
                    "bubblewrap (bwrap) is required for ready-set-encrypt on Linux; \
                     install via `apt install bubblewrap` (Debian/Ubuntu) or \
                     `dnf install bubblewrap` (Fedora). Set `unsandboxed = true` on the \
                     secret to bypass at your own risk."
                        .into(),
                ),
            });
        }
        let mut wrapped = render_bwrap_args(config);
        wrapped.push("--".into());
        wrapped.extend(argv);
        Ok(WrapResult {
            argv: wrapped,
            sandboxed: true,
            platform_label: Some(PLATFORM_LABEL),
        })
    }

    /// Render the bubblewrap argv prefix (everything before `--`). Pure;
    /// testable in isolation.
    ///
    /// Strategy: `--ro-bind /` mounts the whole host root read-only inside
    /// the sandbox, then per-path `--bind` overlays add writable holes for
    /// project_root, tmpdir, ~/.cache, and `extra_write_paths`. This gives
    /// us default-deny-for-writes without an explicit denylist — the macOS
    /// profile's `(deny file-write* ~/.ssh ...)` is implicit on Linux
    /// because the base mount is read-only.
    #[must_use]
    pub fn render_bwrap_args(config: &SandboxConfig) -> Vec<String> {
        let mut argv: Vec<String> = Vec::new();
        argv.push("bwrap".into());
        // Tear down the sandbox if the dispatcher dies.
        argv.push("--die-with-parent".into());
        // Network is allowed by default (provider CLIs need outbound HTTPS).
        // Use --unshare-pid for process isolation but NOT --unshare-net.
        argv.push("--unshare-pid".into());
        // Read-only host root as the base layer.
        argv.push("--ro-bind".into());
        argv.push("/".into());
        argv.push("/".into());
        // Per-path writable overlays.
        let home = home_dir().display().to_string();
        let project_root = config.project_root.display().to_string();
        let tmpdir = std::env::temp_dir().display().to_string();
        for p in [&project_root, &tmpdir] {
            argv.push("--bind".into());
            argv.push(p.clone());
            argv.push(p.clone());
        }
        let cache_dir = format!("{home}/.cache");
        argv.push("--bind".into());
        argv.push(cache_dir.clone());
        argv.push(cache_dir);
        for extra in &config.extra_write_paths {
            let expanded = expand_tilde(extra, &home);
            argv.push("--bind".into());
            argv.push(expanded.clone());
            argv.push(expanded);
        }
        // Minimal /dev and /proc.
        argv.push("--dev".into());
        argv.push("/dev".into());
        argv.push("--proc".into());
        argv.push("/proc".into());
        argv
    }
}

#[cfg(all(test, target_os = "linux"))]
use linux::render_bwrap_args;

// ---------------------------------------------------------------------------
// Windows backend
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod windows {
    use super::{Error, PathBuf, Result, SandboxConfig, WrapResult, expand_tilde, home_dir};

    /// Stable label written to audit entries identifying the Windows backend.
    pub const PLATFORM_LABEL: &str = "windows-appcontainer";

    /// Binary name of the `AppContainer` launcher. Lives next to
    /// `ready-set-encrypt.exe` in the install dir; the plugin locates it
    /// via PATH walk.
    pub const LAUNCHER_BINARY: &str = "ready-set-encrypt-launcher.exe";

    pub fn wrap(argv: Vec<String>, config: &SandboxConfig) -> Result<WrapResult> {
        let launcher = which::which(LAUNCHER_BINARY).map_err(|_| Error::MissingDependency {
            name: LAUNCHER_BINARY.into(),
            hint: Some(
                "ready-set-encrypt-launcher.exe is required for the Windows \
                 AppContainer sandbox; install it alongside the plugin or set \
                 `unsandboxed = true` on the secret to bypass at your own risk"
                    .into(),
            ),
        })?;
        let mut wrapped = render_launcher_args(&launcher, config);
        wrapped.push("--".into());
        wrapped.extend(argv);
        Ok(WrapResult {
            argv: wrapped,
            sandboxed: true,
            platform_label: Some(PLATFORM_LABEL),
        })
    }

    /// Render the launcher argv prefix (everything before `--`). Pure;
    /// testable in isolation.
    ///
    /// Layout: `[launcher_exe, --project-root, <root>, --tmpdir, <tmp>,
    /// --cache, <home>/AppData/Local/Cache, --extra-write, <path>...,
    /// --container-name, <stable-name>]`. The launcher binary then
    /// creates/looks up the `AppContainer` SID by that name, grants
    /// per-path write ACLs, and spawns the child via `CreateProcessW`
    /// with `STARTUPINFOEXW` carrying `SECURITY_CAPABILITIES`.
    #[must_use]
    pub fn render_launcher_args(launcher: &PathBuf, config: &SandboxConfig) -> Vec<String> {
        let mut argv: Vec<String> = Vec::new();
        argv.push(launcher.display().to_string());
        let home = home_dir().display().to_string();
        let project_root = config.project_root.display().to_string();
        let tmpdir = std::env::temp_dir().display().to_string();
        argv.push("--project-root".into());
        argv.push(project_root);
        argv.push("--tmpdir".into());
        argv.push(tmpdir);
        argv.push("--cache".into());
        argv.push(format!("{home}\\AppData\\Local\\Cache"));
        for extra in &config.extra_write_paths {
            argv.push("--extra-write".into());
            argv.push(expand_tilde(extra, &home));
        }
        argv.push("--container-name".into());
        argv.push(container_name(&config.project_root));
        argv
    }

    /// Stable `AppContainer` profile name derived from the project root.
    /// Re-derivable by anyone with the same root path; idempotent across
    /// rotations of the same secret.
    fn container_name(project_root: &std::path::Path) -> String {
        use ready_set_sdk::fs::sha256_bytes;
        let key = project_root.display().to_string();
        let hash = sha256_bytes(key.as_bytes());
        // AppContainer names: ≤ 64 chars, ASCII alphanumeric + '.'. Take
        // the leading 32 hex chars; prefix to keep it human-greppable in
        // tooling.
        format!("ready-set-encrypt.{}", &hash[..32])
    }
}

#[cfg(all(test, target_os = "windows"))]
use windows::render_launcher_args;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn expand_tilde(path: &Path, home: &str) -> String {
    let s = path.display().to_string();
    let Some(rest) = s.strip_prefix("~/") else {
        return if s == "~" { home.to_owned() } else { s };
    };
    format!("{home}/{rest}")
}

#[cfg(target_os = "macos")]
fn escape_path(path: &str) -> String {
    // sandbox-exec's TinyScheme dialect needs backslashes and double-quotes
    // escaped inside string literals. Filesystem paths very rarely contain
    // either, but escaping defensively keeps us correct for unusual project
    // roots (e.g. a tempdir on macOS Sequoia under /var/folders/...).
    path.replace('\\', "\\\\").replace('"', "\\\"")
}

fn home_dir() -> PathBuf {
    directories::UserDirs::new().map_or_else(|| PathBuf::from("/"), |d| d.home_dir().to_path_buf())
}

#[cfg(all(test, target_os = "macos"))]
mod tests_macos {
    use super::*;

    fn config_with_extras(extras: Vec<&str>) -> SandboxConfig {
        SandboxConfig {
            project_root: PathBuf::from("/tmp/proj"),
            extra_write_paths: extras.into_iter().map(PathBuf::from).collect(),
            unsandboxed: false,
        }
    }

    #[test]
    fn render_substitutes_all_placeholders() {
        let cfg = config_with_extras(vec![]);
        let rendered = render_profile(&cfg);
        assert!(!rendered.contains("<PROJECT_ROOT>"));
        assert!(!rendered.contains("<TMPDIR>"));
        assert!(!rendered.contains("<HOME>"));
        assert!(!rendered.contains("<EXTRA_WRITES>"));
        assert!(rendered.contains("(subpath \"/tmp/proj\")"));
        assert!(rendered.contains("(deny default)"));
        assert!(rendered.contains("(allow network*)"));
    }

    #[test]
    fn render_includes_extra_write_paths_with_tilde_expansion() {
        let cfg = config_with_extras(vec!["~/.fly", "/opt/state"]);
        let rendered = render_profile(&cfg);
        let home = home_dir().display().to_string();
        assert!(
            rendered.contains(&format!("(subpath \"{home}/.fly\")")),
            "expected ~ expansion, got: {rendered}"
        );
        assert!(rendered.contains("(subpath \"/opt/state\")"));
    }

    #[test]
    fn render_includes_denylist_for_shell_init_files() {
        let cfg = config_with_extras(vec![]);
        let rendered = render_profile(&cfg);
        let home = home_dir().display().to_string();
        assert!(rendered.contains(&format!("(literal \"{home}/.zshrc\")")));
        assert!(rendered.contains(&format!("(literal \"{home}/.bashrc\")")));
        assert!(rendered.contains(&format!("(subpath \"{home}/.ssh\")")));
    }

    #[test]
    fn unsandboxed_returns_argv_unchanged() {
        let cfg = SandboxConfig {
            project_root: PathBuf::from("/x"),
            extra_write_paths: vec![],
            unsandboxed: true,
        };
        let argv = vec!["echo".to_owned(), "hi".to_owned()];
        let result = wrap(argv.clone(), &cfg).unwrap();
        assert_eq!(result.argv, argv);
        assert!(!result.sandboxed);
        assert!(result.platform_label.is_none());
    }

    #[test]
    fn wrap_prepends_sandbox_exec_and_profile() {
        let cfg = config_with_extras(vec![]);
        let argv = vec!["tee".to_owned(), "result.txt".to_owned()];
        let result = wrap(argv.clone(), &cfg).unwrap();
        assert!(result.sandboxed);
        assert_eq!(result.platform_label, Some(PLATFORM_LABEL));
        assert_eq!(result.argv[0], "sandbox-exec");
        assert_eq!(result.argv[1], "-p");
        assert!(result.argv[2].contains("(deny default)"));
        assert_eq!(result.argv[3], "--");
        assert_eq!(&result.argv[4..], &argv[..]);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests_linux {
    use super::*;

    fn config_with_extras(extras: Vec<&str>) -> SandboxConfig {
        SandboxConfig {
            project_root: PathBuf::from("/tmp/proj"),
            extra_write_paths: extras.into_iter().map(PathBuf::from).collect(),
            unsandboxed: false,
        }
    }

    #[test]
    fn render_bwrap_args_includes_ro_root_and_writable_overlays() {
        let cfg = config_with_extras(vec![]);
        let argv = render_bwrap_args(&cfg);
        assert_eq!(argv[0], "bwrap");
        assert!(argv.contains(&"--die-with-parent".to_owned()));
        assert!(argv.contains(&"--unshare-pid".to_owned()));
        // --ro-bind / / appears as a triple
        let pos = argv.iter().position(|a| a == "--ro-bind").unwrap();
        assert_eq!(argv[pos + 1], "/");
        assert_eq!(argv[pos + 2], "/");
        // project_root is bound writable
        assert!(
            argv.windows(3)
                .any(|w| w[0] == "--bind" && w[1] == "/tmp/proj" && w[2] == "/tmp/proj")
        );
    }

    #[test]
    fn render_bwrap_args_expands_tilde_in_extras() {
        let cfg = config_with_extras(vec!["~/.fly"]);
        let argv = render_bwrap_args(&cfg);
        let home = home_dir().display().to_string();
        let expected = format!("{home}/.fly");
        assert!(
            argv.windows(3)
                .any(|w| w[0] == "--bind" && w[1] == expected && w[2] == expected),
            "expected ~ expansion to {expected}, got: {argv:?}"
        );
    }

    #[test]
    fn unsandboxed_returns_argv_unchanged() {
        let cfg = SandboxConfig {
            project_root: PathBuf::from("/x"),
            extra_write_paths: vec![],
            unsandboxed: true,
        };
        let argv = vec!["echo".to_owned(), "hi".to_owned()];
        let result = wrap(argv.clone(), &cfg).unwrap();
        assert_eq!(result.argv, argv);
        assert!(!result.sandboxed);
        assert!(result.platform_label.is_none());
    }
}
