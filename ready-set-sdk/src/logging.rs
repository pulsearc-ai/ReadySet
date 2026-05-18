//! `tracing` setup honoring the dispatcher env contract.

use std::sync::OnceLock;

use tracing_subscriber::{EnvFilter, fmt};

use crate::context::{ColorMode, Context, LogLevel};

static INSTALLED: OnceLock<()> = OnceLock::new();

/// Install a tracing subscriber configured from `ctx`.
///
/// Idempotent: subsequent calls are no-ops. Honors `--quiet` (errors only),
/// `--verbose` (debug), and the color preference. Also respects the
/// `RUST_LOG` env var if set, in which case it takes precedence over
/// `READY_SET_LOG`.
pub fn install(ctx: &Context) {
    INSTALLED.get_or_init(|| install_inner(ctx));
}

fn install_inner(ctx: &Context) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        let level = match ctx.log_level() {
            LogLevel::Quiet => "error",
            LogLevel::Normal => "info",
            LogLevel::Verbose => "debug",
        };
        EnvFilter::new(level)
    });

    let want_color = match ctx.color() {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => is_terminal::IsTerminal::is_terminal(&std::io::stderr()),
    };

    let subscriber = fmt()
        .with_env_filter(env_filter)
        .with_writer(std::io::stderr)
        .with_ansi(want_color)
        .with_target(false)
        .finish();
    let _ = tracing::subscriber::set_global_default(subscriber);
}
