//! Configuration tests: layer precedence, the `APP_*` environment
//! convention, environment semantics, validation invariants, dotenv
//! handling and connection-URL assembly.
//!
//! Environment variables and the working directory are process-global
//! state, so every test touching either goes through `temp-env`, which
//! serializes access and restores the previous values — and every load
//! starts from a fully unset `APP_*` namespace, so the ambient
//! environment (a `.env` exported by `just`, a stray shell export) can
//! never leak into an assertion.

use common::config::{Config, Environment, Postgresql, PostgresqlPool, Server};
use secrecy::{ExposeSecret, SecretString};

/// Every documented `APP_*` variable, unset as a baseline before each
/// load.
const ALL_VARS: &[&str] = &[
    "APP_NAME",
    "APP_VERSION",
    "APP_ENVIRONMENT",
    "APP_DEBUG",
    "APP_SERVER__HOST",
    "APP_SERVER__PORT",
    "APP_SERVER__TIMEOUT",
    "APP_DATABASE__HOST",
    "APP_DATABASE__PORT",
    "APP_DATABASE__USERNAME",
    "APP_DATABASE__PASSWORD",
    "APP_DATABASE__DATABASE",
    "APP_DATABASE__POOL__MAX_CONNECTIONS",
    "APP_DATABASE__POOL__MIN_CONNECTIONS",
    "APP_DATABASE__POOL__CONNECT_TIMEOUT",
    "APP_DATABASE__POOL__ACQUIRE_TIMEOUT",
    "APP_DATABASE__POOL__IDLE_TIMEOUT",
    "APP_DATABASE__POOL__MAX_LIFETIME",
];

/// Unsets the whole documented namespace, then applies `vars` on top,
/// for the duration of `body` (serialized by `temp-env`'s global lock).
fn with_hermetic_env<R>(vars: &[(&str, Option<&str>)], body: impl FnOnce() -> R) -> R {
    let mut merged: Vec<(String, Option<String>)> = ALL_VARS
        .iter()
        .map(|key| ((*key).to_owned(), None))
        .collect();
    for (key, value) in vars {
        let value = value.map(str::to_owned);
        match merged.iter_mut().find(|(existing, _)| existing == key) {
            Some(slot) => slot.1 = value,
            None => merged.push(((*key).to_owned(), value)),
        }
    }

    temp_env::with_vars(merged, body)
}

/// Loads the configuration with the given variables applied over the
/// hermetic baseline, expecting success.
fn load_with(vars: &[(&str, Option<&str>)]) -> Config {
    try_load_with(vars).expect("configuration must load")
}

/// Same as [`load_with`] for tests asserting the failure path.
fn try_load_with(vars: &[(&str, Option<&str>)]) -> anyhow::Result<Config> {
    with_hermetic_env(vars, || Config::load("under-test", "0.0.0"))
}

/// Restores the working directory when dropped, panic included, so a
/// failing file-layer test cannot strand the whole binary in a scratch
/// directory.
struct CwdGuard(std::path::PathBuf);

impl CwdGuard {
    /// Remembers the current directory, then enters `target`.
    fn enter(target: &std::path::Path) -> Self {
        let previous = std::env::current_dir().expect("the working directory must be readable");
        std::env::set_current_dir(target).expect("the target directory must exist");

        Self(previous)
    }
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

/// Creates a fresh scratch directory for tests reading files from the
/// working directory (the `app-config.json` layer, `load_dotenv`).
fn scratch_dir(label: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("starterhub-config-{label}-{}", std::process::id()));
    // Recreate from scratch so a previous run's files never leak in.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory must be creatable");

    dir
}

/// With no external source, every documented default holds and the boot
/// requires zero setup.
#[test]
fn defaults_require_zero_setup() {
    let config = load_with(&[]);

    assert_eq!(config.name, "under-test");
    assert_eq!(config.version, "0.0.0");
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
        ("APP_DATABASE__PASSWORD", Some("s3cret")),
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
    assert!(try_load_with(&[("APP_SERVER__PORT", Some("not-a-port"))]).is_err());
}

/// String-typed values that merely look numeric survive intact.
/// Regression test: source-level `try_parsing` round-tripped them
/// through numbers, silently turning a password of "0123" into "123".
#[test]
fn string_values_survive_numeric_lookalikes() {
    let config = load_with(&[
        ("APP_DATABASE__PASSWORD", Some("0123")),
        ("APP_NAME", Some("007")),
        ("APP_VERSION", Some("1.10")),
    ]);

    assert_eq!(config.database.password.expose_secret(), "0123");
    assert_eq!(config.name, "007");
    assert_eq!(config.version, "1.10");
}

/// An unknown key aborts the load: the `APP_*` namespace belongs to the
/// application, so a typoed override (here a misspelled password key
/// that would silently leave the default in place) must fail loudly.
#[test]
fn unknown_keys_abort_the_load() {
    assert!(try_load_with(&[("APP_DATABASE__PASWORD", Some("real-secret"))]).is_err());
    assert!(try_load_with(&[("APP_SEVER__PORT", Some("1234"))]).is_err());
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
        let config = load_with(&[
            ("APP_ENVIRONMENT", Some(spelling)),
            // Production refuses the default password; irrelevant to
            // the spelling under test, so satisfy it uniformly.
            ("APP_DATABASE__PASSWORD", Some("s3cret")),
        ]);

        assert_eq!(config.environment, expected, "spelling {spelling:?}");
    }

    for spelling in ["LOCAL", "PRODUCTION", "dev", "DEV", "prod", "Staging", ""] {
        assert!(
            try_load_with(&[("APP_ENVIRONMENT", Some(spelling))]).is_err(),
            "spelling {spelling:?} must be rejected"
        );
    }
}

/// Nonsensical cross-field values abort the load instead of booting
/// into a broken state (instant timeouts, an unsatisfiable pool).
#[test]
fn invalid_invariants_abort_the_load() {
    assert!(try_load_with(&[("APP_SERVER__TIMEOUT", Some("0"))]).is_err());
    assert!(try_load_with(&[("APP_DATABASE__POOL__CONNECT_TIMEOUT", Some("0"))]).is_err());
    assert!(try_load_with(&[("APP_DATABASE__POOL__ACQUIRE_TIMEOUT", Some("0"))]).is_err());
    assert!(try_load_with(&[("APP_DATABASE__POOL__MIN_CONNECTIONS", Some("101"))]).is_err());
}

/// Production never boots on the template's out-of-the-box credentials:
/// forgetting APP_DATABASE__PASSWORD in a deployment must fail loudly,
/// not silently authenticate as root/root.
#[test]
fn production_refuses_the_default_database_password() {
    assert!(try_load_with(&[("APP_ENVIRONMENT", Some("production"))]).is_err());

    let config = load_with(&[
        ("APP_ENVIRONMENT", Some("production")),
        ("APP_DATABASE__PASSWORD", Some("s3cret")),
    ]);

    assert_eq!(config.environment, Environment::Production);
}

/// The working-directory `app-config.json` sits between the defaults
/// and the environment: it overrides the former and loses to the
/// latter.
#[test]
fn file_layer_sits_between_defaults_and_env() {
    let dir = scratch_dir("layers");
    std::fs::write(
        dir.join("app-config.json"),
        r#"{ "debug": true, "server": { "port": 7777 } }"#,
    )
    .expect("the override file must be writable");

    with_hermetic_env(&[("APP_DEBUG", Some("false"))], || {
        let _cwd = CwdGuard::enter(&dir);
        let config = Config::load("under-test", "0.0.0").expect("configuration must load");

        assert_eq!(
            config.server.port, 7777,
            "the file layer must beat the default"
        );
        assert!(!config.debug, "the env layer must beat the file");
    });
}

/// A present but malformed `app-config.json` aborts the load: a file
/// someone wrote must never be silently skipped.
#[test]
fn malformed_file_layer_fails_the_load() {
    let dir = scratch_dir("malformed-file");
    std::fs::write(dir.join("app-config.json"), "{ not json").expect("the file must be writable");

    with_hermetic_env(&[], || {
        let _cwd = CwdGuard::enter(&dir);

        assert!(Config::load("under-test", "0.0.0").is_err());
    });
}

/// `Config::load` never reads `.env` on its own — that is the explicit
/// [`common::config::load_dotenv`] helper's job, called by the binaries.
/// The split is what keeps this whole test file hermetic.
#[test]
fn dotenv_is_loaded_by_the_helper_not_the_library() {
    let dir = scratch_dir("dotenv");
    std::fs::write(dir.join(".env"), "APP_DEBUG=true\n").expect(".env must be writable");

    with_hermetic_env(&[], || {
        let _cwd = CwdGuard::enter(&dir);

        let untouched = Config::load("under-test", "0.0.0").expect("configuration must load");
        assert!(!untouched.debug, "the library must ignore .env");

        common::config::load_dotenv().expect("a valid .env must load");
        let applied = Config::load("under-test", "0.0.0").expect("configuration must load");
        assert!(applied.debug, "the helper must apply .env");
    });
}

/// A missing `.env` is a normal setup; a malformed one aborts instead
/// of being silently half-read (regression: INI-style `;` comments).
#[test]
fn dotenv_missing_is_fine_and_malformed_aborts() {
    let dir = scratch_dir("dotenv-bad");

    with_hermetic_env(&[], || {
        let _cwd = CwdGuard::enter(&dir);

        assert!(
            common::config::load_dotenv().is_ok(),
            "missing .env is normal"
        );

        std::fs::write(".env", "; ini comments are not dotenv\n").expect(".env must be writable");
        assert!(
            common::config::load_dotenv().is_err(),
            "malformed .env must abort"
        );
    });
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

/// IPv6 hosts are bracketed everywhere a `host:port` pair is assembled:
/// a bare colon would be read as the port separator.
#[test]
fn ipv6_hosts_are_bracketed() {
    let server = Server {
        host: "::".to_owned(),
        port: 8080,
        timeout: 30,
    };
    assert_eq!(server.url(), "[::]:8080");

    let database = Postgresql {
        host: "::1".to_owned(),
        port: 5432,
        username: "root".to_owned(),
        password: SecretString::from("root"),
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
        "postgres://root:root@[::1]:5432/app"
    );
}
