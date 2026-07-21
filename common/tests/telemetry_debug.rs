//! Telemetry bootstrap, debug flavour.
//!
//! Lives in its own integration-test binary on purpose: the global
//! subscriber installs once per process, so each `init` scenario needs
//! its own process — which is exactly what separate files under `tests/`
//! provide. This binary covers `debug = true` with `RUST_LOG` unset (the
//! built-in default level applies).

/// `init(true)` installs the subscriber with the DEBUG default level
/// when `RUST_LOG` is not set.
#[test]
fn init_debug_without_rust_log() {
    temp_env::with_var_unset("RUST_LOG", || common::telemetry::init(true));
}
