//! Error and result types used throughout the SDK.

use std::fmt;

/// SDK-wide [`Result`] alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors produced by SDK helpers.
///
/// `Error` is `#[non_exhaustive]`: callers must use a wildcard arm in `match`
/// statements. Adding a new variant in a future minor release is non-breaking.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// An I/O failure surfaced by `std::io`.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A TOML document failed to parse or did not match the expected schema.
    #[error("toml: {0}")]
    TomlParse(String),

    /// A JSON document failed to parse or did not match the expected schema.
    #[error("json: {0}")]
    JsonParse(String),

    /// A required external dependency was not found.
    #[error("missing dependency: {name}{}", .hint.as_deref().map(|h| format!(" ({h})")).unwrap_or_default())]
    MissingDependency {
        /// Human-readable name of the missing dependency.
        name: String,
        /// Optional installation hint shown to the user.
        hint: Option<String>,
    },

    /// A plugin or peer violated the documented dispatcher↔plugin contract.
    #[error("contract violation: {0}")]
    ContractViolation(String),

    /// A catch-all variant for errors that do not fit the categories above.
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// Construct an `Other` error from anything that implements [`fmt::Display`].
    pub fn other(msg: impl fmt::Display) -> Self {
        Self::Other(msg.to_string())
    }

    /// Construct a `ContractViolation` error from anything that implements
    /// [`fmt::Display`].
    pub fn contract(msg: impl fmt::Display) -> Self {
        Self::ContractViolation(msg.to_string())
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Self::TomlParse(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::JsonParse(value.to_string())
    }
}
