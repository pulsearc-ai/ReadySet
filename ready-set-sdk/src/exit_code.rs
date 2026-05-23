//! Documented process exit codes.
//!
//! Mirrors
//! [`docs/contracts/exit-codes.md`](https://github.com/pulsearc-ai/ReadySet/blob/main/docs/contracts/exit-codes.md)
//! exactly. Adding a variant requires a corresponding entry in that contract
//! document.

use crate::error::Error;

/// Documented process exit codes returned by `ready-set` commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ExitCode {
    /// Success.
    Ok,
    /// The user's input was invalid.
    UserError,
    /// An I/O, permission, or environmental error.
    SystemError,
    /// A required external tool was not found on PATH.
    DependencyMissing,
    /// A command requiring a cargo workspace was invoked outside one.
    NotCargoWorkspace,
    /// A plugin violated the dispatcher↔plugin contract.
    ContractViolation,
    /// The dispatcher could not resolve the requested subcommand.
    UnknownSubcommand,
    /// A child process was terminated by signal `N`. The numeric exit
    /// code emitted to the OS is `128 + N`, following the POSIX shell
    /// convention. Only meaningful on Unix; Windows children always
    /// have `ExitStatus::code() == Some(_)`.
    Signaled(u8),
}

impl ExitCode {
    /// Return the numeric exit code as a `u8`.
    ///
    /// `Signaled(n)` returns `128 + n`, saturating at `255` for
    /// hypothetical signal numbers that would overflow.
    #[must_use]
    pub const fn as_u8(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::UserError => 1,
            Self::SystemError => 2,
            Self::DependencyMissing => 3,
            Self::NotCargoWorkspace => 4,
            Self::ContractViolation => 5,
            Self::UnknownSubcommand => 127,
            Self::Signaled(n) => 128_u8.saturating_add(n),
        }
    }
}

impl From<ExitCode> for std::process::ExitCode {
    fn from(value: ExitCode) -> Self {
        Self::from(value.as_u8())
    }
}

impl From<&Error> for ExitCode {
    fn from(value: &Error) -> Self {
        match value {
            Error::TomlParse(_) | Error::JsonParse(_) => Self::UserError,
            Error::MissingDependency { .. } => Self::DependencyMissing,
            Error::ContractViolation(_) => Self::ContractViolation,
            // `Error` is `#[non_exhaustive]`; the wildcard catches both the
            // current `Io`/`Other` variants and any added in future minor
            // releases.
            _ => Self::SystemError,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_values_match_contract() {
        assert_eq!(ExitCode::Ok.as_u8(), 0);
        assert_eq!(ExitCode::UserError.as_u8(), 1);
        assert_eq!(ExitCode::SystemError.as_u8(), 2);
        assert_eq!(ExitCode::DependencyMissing.as_u8(), 3);
        assert_eq!(ExitCode::NotCargoWorkspace.as_u8(), 4);
        assert_eq!(ExitCode::ContractViolation.as_u8(), 5);
        assert_eq!(ExitCode::UnknownSubcommand.as_u8(), 127);
        assert_eq!(ExitCode::Signaled(0).as_u8(), 128);
        assert_eq!(ExitCode::Signaled(2).as_u8(), 130); // SIGINT
        assert_eq!(ExitCode::Signaled(15).as_u8(), 143); // SIGTERM
        assert_eq!(ExitCode::Signaled(255).as_u8(), 255); // saturates
    }

    #[test]
    fn maps_errors_to_codes() {
        let io = Error::Io(std::io::Error::other("nope"));
        assert_eq!(ExitCode::from(&io), ExitCode::SystemError);

        let dep = Error::MissingDependency {
            name: "git".into(),
            hint: None,
        };
        assert_eq!(ExitCode::from(&dep), ExitCode::DependencyMissing);

        let contract = Error::contract("bad");
        assert_eq!(ExitCode::from(&contract), ExitCode::ContractViolation);

        let toml = Error::TomlParse("oops".into());
        assert_eq!(ExitCode::from(&toml), ExitCode::UserError);
    }
}
