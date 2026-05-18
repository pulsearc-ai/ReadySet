//! `__describe` subcommand support.
//!
//! See `docs/contracts/describe.md` for the source of truth.

use std::ffi::OsString;
use std::io::Write;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilityDescriptor;
use crate::error::{Error, Result};
use crate::exit_code::ExitCode;

/// Stability tier reported by `__describe` and the manifest sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stability {
    /// Stable, follows semver.
    Stable,
    /// May break in any release.
    Experimental,
    /// Slated for removal.
    Deprecated,
}

/// Operating system enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Linux family.
    Linux,
    /// macOS.
    Macos,
    /// Windows.
    Windows,
}

impl Platform {
    /// The platform of the running binary, if known.
    #[must_use]
    pub const fn current() -> Option<Self> {
        if cfg!(target_os = "linux") {
            Some(Self::Linux)
        } else if cfg!(target_os = "macos") {
            Some(Self::Macos)
        } else if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else {
            None
        }
    }
}

/// Plugin self-description payload.
///
/// Same shape as [`crate::manifest::Manifest`]; the two share a JSON schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Describe {
    /// One-line summary, max 80 chars.
    pub description: String,
    /// Plugin semver.
    pub version: semver::Version,
    /// Stability tier.
    pub stability: Stability,
    /// Minimum dispatcher semver this plugin requires.
    pub min_dispatcher_version: semver::Version,
    /// Supported operating systems.
    pub platforms: Vec<Platform>,
    /// Whether the plugin requires a cargo workspace context.
    pub requires_cargo_workspace: bool,
    /// Product capabilities contributed by this plugin.
    pub capabilities: Vec<CapabilityDescriptor>,
}

impl Describe {
    /// Print this `Describe` as a single line of JSON to stdout, terminated
    /// by a newline.
    ///
    /// # Errors
    ///
    /// Returns [`Error::JsonParse`] if serialization fails or [`Error::Io`]
    /// if writing to stdout fails.
    pub fn emit_stdout(&self) -> Result<()> {
        let line = serde_json::to_string(self)?;
        let stdout = std::io::stdout();
        let mut handle = stdout.lock();
        writeln!(handle, "{line}").map_err(Error::Io)?;
        handle.flush().map_err(Error::Io)?;
        Ok(())
    }

    /// If the first non-program argument is `__describe`, emit this `Describe`
    /// and return `Some(ExitCode::Ok)`. Otherwise return `None` so the caller
    /// can continue with normal argument parsing.
    ///
    /// Plugins MUST call this before their main argument parser runs to
    /// avoid clap rejecting the unknown subcommand.
    ///
    /// # Errors
    ///
    /// Returns `Some(ExitCode::ContractViolation)` if `__describe` is given
    /// extra arguments, or if emitting the JSON line fails. Returns
    /// `Some(ExitCode::Ok)` on the happy path.
    pub fn handle_arg0_describe(
        &self,
        args: impl IntoIterator<Item = OsString>,
    ) -> Option<ExitCode> {
        let mut iter = args.into_iter();
        // Skip program name (argv[0]).
        drop(iter.next());
        let first = iter.next()?;
        if first != "__describe" {
            return None;
        }
        if iter.next().is_some() {
            // Extra args after __describe violate the contract.
            eprintln!("__describe accepts no extra arguments");
            return Some(ExitCode::ContractViolation);
        }
        match self.emit_stdout() {
            Ok(()) => Some(ExitCode::Ok),
            Err(_) => Some(ExitCode::ContractViolation),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Describe {
        Describe {
            description: "test".into(),
            version: semver::Version::new(0, 1, 0),
            stability: Stability::Stable,
            min_dispatcher_version: semver::Version::new(0, 1, 0),
            platforms: vec![Platform::Linux, Platform::Macos, Platform::Windows],
            requires_cargo_workspace: false,
            capabilities: Vec::new(),
        }
    }

    #[test]
    fn skips_when_no_describe_arg() {
        let d = fixture();
        let args: Vec<OsString> = ["prog", "run", "--flag"]
            .iter()
            .map(OsString::from)
            .collect();
        let result = d.handle_arg0_describe(args);
        assert!(result.is_none());
    }

    #[test]
    fn skips_when_no_args_at_all() {
        let d = fixture();
        let result = d.handle_arg0_describe([OsString::from("prog")]);
        assert!(result.is_none());
    }

    #[test]
    fn rejects_extra_args() {
        let d = fixture();
        let args: Vec<OsString> = ["prog", "__describe", "extra"]
            .iter()
            .map(OsString::from)
            .collect();
        let result = d.handle_arg0_describe(args);
        assert_eq!(result, Some(ExitCode::ContractViolation));
    }

    #[test]
    fn json_round_trip() {
        let d = fixture();
        let json = serde_json::to_string(&d).unwrap();
        let back: Describe = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
        assert!(json.contains("\"capabilities\":[]"));
    }

    #[test]
    fn rejects_json_without_capabilities() {
        let json = r#"{"description":"test","version":"0.1.0","stability":"stable","min_dispatcher_version":"0.1.0","platforms":["linux"],"requires_cargo_workspace":false}"#;
        assert!(serde_json::from_str::<Describe>(json).is_err());
    }
}
