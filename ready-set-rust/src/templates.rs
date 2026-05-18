//! Embedded template files written by the Rust provider.

/// `rust-toolchain.toml`.
pub const RUST_TOOLCHAIN: &str = include_str!("templates/rust-toolchain.toml");

/// `rustfmt.toml`.
pub const RUSTFMT: &str = include_str!("templates/rustfmt.toml");

/// `clippy.toml`.
pub const CLIPPY: &str = include_str!("templates/clippy.toml");

/// `.gitignore` content used inside the managed block.
pub const GITIGNORE: &str = include_str!("templates/gitignore");

/// `[workspace.lints.*]` block written into the root `Cargo.toml`.
pub const WORKSPACE_LINTS: &str = include_str!("templates/workspace-lints.toml");

/// `.gitignore` managed-block opening marker.
pub const GITIGNORE_BEGIN: &str = "# >>> ready-set managed >>>";

/// `.gitignore` managed-block closing marker.
pub const GITIGNORE_END: &str = "# <<< ready-set managed <<<";
