//! Tracing/logging initialisation.

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Installs the global tracing subscriber.
///
/// The default verbosity is driven by the `debug` configuration flag
/// (`APP_DEBUG`): DEBUG level when `true`, INFO level otherwise (tracing has
/// no NOTICE level, INFO is its closest equivalent). `RUST_LOG` still takes
/// precedence when set, so verbosity remains tunable per module at runtime,
/// e.g. `RUST_LOG=sqlx=warn,debug`.
///
/// Built as a layered registry so extra layers (JSON output,
/// OpenTelemetry, ...) can be stacked without touching the callers.
///
/// # Panics
///
/// Panics if called twice: the subscriber can only be installed once.
pub fn init(debug: bool) {
    // Translate the configuration flag into a filter directive; INFO is the
    // quiet default, DEBUG the opt-in verbose mode.
    let default_level = if debug {
        LevelFilter::DEBUG
    } else {
        LevelFilter::INFO
    };

    tracing_subscriber::registry()
        // `RUST_LOG` wins when present: an operator must be able to raise
        // verbosity per module at runtime without editing configuration.
        // Lossy parse on purpose: one invalid directive in an incident
        // commander's RUST_LOG must not silently throw away the whole
        // filter — the valid directives survive and the invalid one is
        // reported on stderr.
        .with(
            EnvFilter::builder()
                .with_default_directive(default_level.into())
                .from_env_lossy(),
        )
        // Human-readable console output; add JSON/OTel layers here later
        // without touching any call site.
        .with(fmt::layer())
        .init();
}
