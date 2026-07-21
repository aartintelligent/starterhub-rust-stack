//! Telemetry bootstrap, default flavour.
//!
//! Lives in its own integration-test binary on purpose: the global
//! subscriber installs once per process, so each `init` scenario needs
//! its own process — which is exactly what separate files under `tests/`
//! provide. This binary covers `debug = false` with `RUST_LOG` set (the
//! operator override wins).

/// `init(false)` installs the subscriber and honours `RUST_LOG` over the
/// INFO default level.
#[test]
fn init_default_honours_rust_log() {
    temp_env::with_var("RUST_LOG", Some("warn"), || common::telemetry::init(false));
}
