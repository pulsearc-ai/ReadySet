//! Per-invocation context populated from the dispatcher env contract.
//!
//! See
//! [`docs/contracts/env-vars.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/env-vars.md)
//! for the source of truth.

use std::env;
use std::path::{Path, PathBuf};

/// Requested log verbosity. Mirrors `READY_SET_LOG`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    /// Errors only.
    Quiet,
    /// Default.
    #[default]
    Normal,
    /// Debug logging.
    Verbose,
}

impl LogLevel {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some("quiet") => Self::Quiet,
            Some("verbose") => Self::Verbose,
            // Unrecognized values fall back to the default per env-vars contract.
            _ => Self::Normal,
        }
    }
}

/// Color preference. Mirrors `READY_SET_COLOR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorMode {
    /// Emit escape codes only when stdout is a TTY.
    #[default]
    Auto,
    /// Always emit escape codes.
    Always,
    /// Never emit escape codes.
    Never,
}

impl ColorMode {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some("always") => Self::Always,
            Some("never") => Self::Never,
            _ => Self::Auto,
        }
    }
}

/// Per-invocation state, constructed from the dispatcher env contract.
#[derive(Debug, Clone)]
pub struct Context {
    dispatcher_version: Option<semver::Version>,
    project_root: Option<PathBuf>,
    config_path: Option<PathBuf>,
    output_mode: crate::output::OutputMode,
    log_level: LogLevel,
    color: ColorMode,
}

impl Context {
    /// Construct a `Context` by reading the documented `READY_SET_*` env vars.
    ///
    /// Tolerates unset and unrecognized values per the env-vars contract.
    #[must_use]
    pub fn from_env() -> Self {
        let dispatcher_version = env::var("READY_SET_DISPATCHER_VERSION")
            .ok()
            .and_then(|raw| semver::Version::parse(raw.trim()).ok());

        let project_root = env::var_os("READY_SET_PROJECT_ROOT")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let config_path = env::var_os("READY_SET_CONFIG_PATH")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let output_mode =
            crate::output::OutputMode::parse(env::var("READY_SET_OUTPUT").ok().as_deref());
        let log_level = LogLevel::parse(env::var("READY_SET_LOG").ok().as_deref());
        let color = ColorMode::parse(env::var("READY_SET_COLOR").ok().as_deref());

        Self {
            dispatcher_version,
            project_root,
            config_path,
            output_mode,
            log_level,
            color,
        }
    }

    /// Construct a default `Context` for tests or programmatic use.
    #[must_use]
    pub const fn default_for_tests() -> Self {
        Self {
            dispatcher_version: None,
            project_root: None,
            config_path: None,
            output_mode: crate::output::OutputMode::Human,
            log_level: LogLevel::Normal,
            color: ColorMode::Auto,
        }
    }

    /// Version of the dispatcher that invoked this plugin, if any.
    #[must_use]
    pub const fn dispatcher_version(&self) -> Option<&semver::Version> {
        self.dispatcher_version.as_ref()
    }

    /// Resolved project root, if any.
    #[must_use]
    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }

    /// Resolved `.ready-set.toml` path, if any.
    #[must_use]
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Requested output mode.
    #[must_use]
    pub const fn output_mode(&self) -> crate::output::OutputMode {
        self.output_mode
    }

    /// Requested log verbosity.
    #[must_use]
    pub const fn log_level(&self) -> LogLevel {
        self.log_level
    }

    /// Color preference.
    #[must_use]
    pub const fn color(&self) -> ColorMode {
        self.color
    }

    /// Return [`Self::project_root`] if known, otherwise `cwd`.
    ///
    /// # Errors
    ///
    /// Returns the underlying I/O error from `std::env::current_dir` if the
    /// current working directory cannot be resolved.
    pub fn project_root_or_cwd(&self) -> std::io::Result<PathBuf> {
        if let Some(root) = &self.project_root {
            return Ok(root.clone());
        }
        env::current_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log_level_with_fallbacks() {
        assert_eq!(LogLevel::parse(Some("quiet")), LogLevel::Quiet);
        assert_eq!(LogLevel::parse(Some("verbose")), LogLevel::Verbose);
        assert_eq!(LogLevel::parse(Some("normal")), LogLevel::Normal);
        // Forward-compat: unknown values fall back, no panic.
        assert_eq!(LogLevel::parse(Some("bogus")), LogLevel::Normal);
        assert_eq!(LogLevel::parse(None), LogLevel::Normal);
    }

    #[test]
    fn parses_color_mode_with_fallbacks() {
        assert_eq!(ColorMode::parse(Some("always")), ColorMode::Always);
        assert_eq!(ColorMode::parse(Some("never")), ColorMode::Never);
        assert_eq!(ColorMode::parse(Some("auto")), ColorMode::Auto);
        assert_eq!(ColorMode::parse(Some("rainbow")), ColorMode::Auto);
        assert_eq!(ColorMode::parse(None), ColorMode::Auto);
    }
}
