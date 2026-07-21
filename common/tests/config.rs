//! Configuration tests: layer precedence, the `APP_*` environment
//! convention, environment semantics and connection-URL assembly.
//!
//! Environment variables are process-global state, so every test that
//! touches them goes through `temp-env`, which serializes access and
//! restores the previous values.

use common::config::{Config, Environment, Postgresql, PostgresqlPool};
use secrecy::{ExposeSecret, SecretString};

/// Loads the configuration with the given variables set (`Some`) or
/// explicitly unset (`None`) for the duration of the call.
fn load_with(vars: &[(&str, Option<&str>)]) -> Config {
    temp_env::with_vars(vars, || {
        Config::load("under-test", "0.0.0").expect("configuration must load")
    })
}

/// With no external source, every documented default holds and the boot
/// requires zero setup.
#[test]
fn defaults_require_zero_setup() {
    let config = load_with(&[
        ("APP_ENVIRONMENT", None),
        ("APP_DEBUG", None),
        ("APP_SERVER__PORT", None),
    ]);

    assert_eq!(config.name, "under-test");
    assert_eq!(config.environment, Environment::Local);
    assert!(!config.debug);
    assert_eq!(config.server.host, "127.0.0.1");
    assert_eq!(config.server.port, 8080);
    assert_eq!(config.server.url(), "127.0.0.1:8080");
    assert_eq!(config.server.timeout, 30);
    assert_eq!(config.database.pool.idle_timeout, 600);
    assert_eq!(config.database.pool.max_lifetime, 1800);
}

/// The documented convention is a SINGLE underscore after the `APP`
/// prefix and a double underscore for nesting. Regression test: with
/// only `separator("__")` set, the config crate silently required
/// `APP__*` and every documented variable was ignored.
#[test]
fn env_overrides_use_single_underscore_prefix() {
    let config = load_with(&[
        ("APP_ENVIRONMENT", Some("production")),
        ("APP_DEBUG", Some("true")),
        ("APP_SERVER__PORT", Some("9999")),
        ("APP_DATABASE__POOL__MAX_LIFETIME", Some("60")),
    ]);

    assert_eq!(config.environment, Environment::Production);
    assert!(config.debug);
    assert_eq!(config.server.port, 9999);
    assert_eq!(config.database.pool.max_lifetime, 60);
}

/// A malformed override aborts the load with an error instead of booting
/// on a half-read configuration.
#[test]
fn malformed_override_fails_the_load() {
    temp_env::with_vars([("APP_SERVER__PORT", Some("not-a-port"))], || {
        assert!(Config::load("under-test", "0.0.0").is_err());
    });
}

/// The environment is strict: exactly the four canonical lowercase
/// spellings deserialize, every historical alias or case variant
/// aborts the load — a typo must never guess an environment.
#[test]
fn environment_is_strict() {
    for (spelling, expected) in [
        ("local", Environment::Local),
        ("development", Environment::Development),
        ("staging", Environment::Staging),
        ("production", Environment::Production),
    ] {
        let config = load_with(&[("APP_ENVIRONMENT", Some(spelling))]);

        assert_eq!(config.environment, expected, "spelling {spelling:?}");
    }

    for spelling in ["LOCAL", "PRODUCTION", "dev", "DEV", "prod", "Staging", ""] {
        temp_env::with_vars([("APP_ENVIRONMENT", Some(spelling))], || {
            assert!(
                Config::load("under-test", "0.0.0").is_err(),
                "spelling {spelling:?} must be rejected"
            );
        });
    }
}

/// The interactive documentation is exposed in local and development
/// only — staging and production never advertise an API explorer.
#[test]
fn docs_exposure_follows_environment() {
    assert!(Environment::Local.exposes_docs());
    assert!(Environment::Development.exposes_docs());
    assert!(!Environment::Staging.exposes_docs());
    assert!(!Environment::Production.exposes_docs());
}

/// Credentials and database name are percent-encoded into the connection
/// URL, so URL-significant characters survive the round-trip.
#[test]
fn connection_url_percent_encodes_credentials() {
    let database = Postgresql {
        host: "localhost".to_owned(),
        port: 5432,
        username: "us@r".to_owned(),
        password: SecretString::from("p@ss:w/rd%"),
        database: "app".to_owned(),
        pool: PostgresqlPool {
            max_connections: 1,
            min_connections: 1,
            connect_timeout: 1,
            acquire_timeout: 1,
            idle_timeout: 1,
            max_lifetime: 1,
        },
    };

    assert_eq!(
        database.url().expose_secret(),
        "postgres://us%40r:p%40ss%3Aw%2Frd%25@localhost:5432/app"
    );
}
