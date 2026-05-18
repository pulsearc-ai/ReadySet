//! Output formatting helpers shared by every plugin.

use std::io::{self, Write};

use serde::Serialize;

use crate::context::{ColorMode, Context, LogLevel};
use crate::error::{Error, Result};

/// Requested output mode. Mirrors `READY_SET_OUTPUT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputMode {
    /// Human-readable output (default).
    #[default]
    Human,
    /// Machine-readable JSON output.
    Json,
}

impl OutputMode {
    pub(crate) fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim) {
            Some("json") => Self::Json,
            // "human" or anything unrecognized falls back to Human per the env contract.
            _ => Self::Human,
        }
    }
}

/// Sink for plugin output. Plugins write here instead of `println!` so output
/// stays consistent with the requested mode and verbosity.
pub struct Output {
    mode: OutputMode,
    log_level: LogLevel,
    color: ColorMode,
    writer: Box<dyn Write + Send>,
}

impl Output {
    /// Build an `Output` from a [`Context`] and a writer (typically stdout).
    pub fn for_context(ctx: &Context, writer: impl Write + Send + 'static) -> Self {
        Self {
            mode: ctx.output_mode(),
            log_level: ctx.log_level(),
            color: ctx.color(),
            writer: Box::new(writer),
        }
    }

    /// Replace the inner writer. Useful in tests.
    #[must_use]
    pub fn with_writer(mut self, writer: impl Write + Send + 'static) -> Self {
        self.writer = Box::new(writer);
        self
    }

    /// Reported output mode.
    #[must_use]
    pub const fn mode(&self) -> OutputMode {
        self.mode
    }

    /// Reported color preference.
    #[must_use]
    pub const fn color(&self) -> ColorMode {
        self.color
    }

    /// Write a human-readable message terminated by a newline.
    ///
    /// Suppressed when the log level is [`LogLevel::Quiet`] or the output mode
    /// is [`OutputMode::Json`].
    pub fn human(&mut self, msg: &str) {
        if self.mode == OutputMode::Json || self.log_level == LogLevel::Quiet {
            return;
        }
        // Best-effort write; output failures are not actionable for callers.
        drop(writeln!(self.writer, "{msg}"));
    }

    /// Serialize a value as a single line of JSON to the output.
    ///
    /// Always emits regardless of log level (errors are surfaced via
    /// [`Self::error`]).
    ///
    /// # Errors
    ///
    /// Returns an [`Error::JsonParse`] if serialization fails or an
    /// [`Error::Io`] if writing to the underlying writer fails.
    pub fn json<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let line = serde_json::to_string(value)?;
        writeln!(self.writer, "{line}").map_err(Error::Io)?;
        Ok(())
    }

    /// Print an error using the writer's diagnostic conventions.
    pub fn error(&mut self, err: &dyn std::error::Error) {
        match self.mode {
            OutputMode::Json => {
                drop(self.json(&serde_json::json!({
                    "error": err.to_string(),
                })));
            },
            OutputMode::Human => {
                drop(writeln!(self.writer, "error: {err}"));
            },
        }
    }

    /// Flush the underlying writer.
    ///
    /// # Errors
    ///
    /// Forwards I/O failures from the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sink(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn captured(ctx: &Context) -> (Output, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
        let buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        (Output::for_context(ctx, Sink(buf.clone())), buf)
    }

    #[test]
    fn human_writes_one_line() {
        let ctx = Context::default_for_tests();
        let (mut out, buf) = captured(&ctx);
        out.human("hello");
        let s = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert_eq!(s, "hello\n");
    }

    #[test]
    fn json_serializes_value() {
        let ctx = Context::default_for_tests();
        let (mut out, buf) = captured(&ctx);
        out.json(&serde_json::json!({"k": 1})).unwrap();
        let s = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert_eq!(s, "{\"k\":1}\n");
    }
}
