//! Sandbox abstraction for plugins.
//!
//! In v0.1.0 the per-platform implementations are no-op stubs that record
//! declared capabilities only. The trait surface is `stable`; implementations
//! are `experimental` and will gain real enforcement post-v0.1.0.

use crate::error::Result;

/// Capability a plugin may declare to its sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Read files within the project root.
    ReadProject,
    /// Write files within the project root.
    WriteProject,
    /// Read files within the user's home directory.
    ReadHome,
    /// Write files within the user's home directory.
    WriteHome,
    /// Network access.
    Network,
    /// Spawn subprocesses.
    Subprocess,
}

/// A platform sandbox. Plugins declare the capabilities they need, then
/// `enter` to commit to the sandbox.
pub trait Sandbox {
    /// Declare the capabilities this plugin needs.
    fn declare(&mut self, capabilities: &[Capability]);
    /// Enter the sandbox. After this returns, attempts to use undeclared
    /// capabilities should fail.
    ///
    /// # Errors
    ///
    /// Implementation-specific. The v0.1.0 stub never errors.
    fn enter(&self) -> Result<()>;
}

/// A no-op sandbox that records declarations and does not enforce anything.
#[derive(Debug, Default, Clone)]
pub struct NoopSandbox {
    declared: Vec<Capability>,
}

impl NoopSandbox {
    /// Capabilities recorded so far.
    #[must_use]
    pub fn declared(&self) -> &[Capability] {
        &self.declared
    }
}

impl Sandbox for NoopSandbox {
    fn declare(&mut self, capabilities: &[Capability]) {
        self.declared.extend_from_slice(capabilities);
    }

    fn enter(&self) -> Result<()> {
        Ok(())
    }
}

/// Return the sandbox for the current platform.
///
/// In v0.1.0 every platform returns a [`NoopSandbox`].
#[must_use]
pub fn for_current_platform() -> Box<dyn Sandbox> {
    Box::new(NoopSandbox::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noop_sandbox_records_declarations() {
        let mut sb = NoopSandbox::default();
        sb.declare(&[Capability::ReadProject, Capability::Subprocess]);
        sb.enter().unwrap();
        assert_eq!(
            sb.declared(),
            &[Capability::ReadProject, Capability::Subprocess]
        );
    }
}
