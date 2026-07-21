//! Strongly-typed, layered application configuration.
//!
//! Sources are merged in ascending priority:
//!
//! 1. Hard-coded defaults (the application boots with zero external setup).
//! 2. An optional system file `/etc/starterhub-rust-stack/app-config.json` — the FHS
//!    location for a Debian 13 deployment, mounted or baked into the
//!    Docker image.
//! 3. An optional `app-config.json` in the working directory, the local
//!    development override (never committed to the repository).
//! 4. Environment variables prefixed with `APP_`, using `__` as the nesting
//!    separator, e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS=50`.

use config::{Environment, File, FileFormat};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

/// Characters escaped when embedding a value into the connection URL:
/// everything except ASCII alphanumerics and the RFC 3986 unreserved
/// marks (`-`, `.`, `_`, `~`). Escaping more than strictly required is
/// always safe, while an unescaped `@`, `:`, `/` or `%` in a credential
/// silently corrupts the URL.
const URL_ENCODE_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// HTTP server settings (`server.*`).
#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    /// Interface to bind, e.g. `127.0.0.1` or `0.0.0.0`.
    pub host: String,
    /// TCP port to listen on.
    pub port: u16,
}

impl Server {
    /// Bindable address in `host:port` form.
    pub fn url(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Connection pool tuning (`database.pool.*`). Durations are in seconds.
#[derive(Debug, Clone, Deserialize)]
pub struct PostgresqlPool {
    /// Upper bound of open connections.
    pub max_connections: u32,
    /// Connections kept warm even when idle.
    pub min_connections: u32,
    /// Max time to wait when establishing a new connection.
    pub connect_timeout: u64,
    /// Max time to wait for a free connection from the pool.
    pub acquire_timeout: u64,
    /// Idle time after which a connection is closed.
    pub idle_timeout: u64,
    /// Max lifetime of a connection before it is recycled.
    pub max_lifetime: u64,
}

/// PostgreSQL settings (`database.*`), kept split field by field so each
/// value can be overridden independently.
#[derive(Debug, Clone, Deserialize)]
pub struct Postgresql {
    /// Database server hostname.
    pub host: String,
    /// Database server port.
    pub port: u16,
    /// Role used to authenticate.
    pub username: String,
    /// Password of the role, wrapped in [`SecretString`]: `Debug` prints
    /// `REDACTED`, accidental logging is impossible, and reading the value
    /// requires an explicit `expose_secret()` call at the point of use.
    pub password: SecretString,
    /// Database name.
    pub database: String,
    /// Connection pool tuning.
    pub pool: PostgresqlPool,
}

impl Postgresql {
    /// Connection string assembled from the individual fields.
    ///
    /// Returned as a [`SecretString`] because it embeds the password:
    /// the secret stays protected end to end, and only the final consumer
    /// (the connection builder) exposes it, at the last possible moment.
    ///
    /// Username, password and database name are percent-encoded so
    /// credentials containing URL-significant characters (`@`, `:`, `/`,
    /// `%`, ...) survive the round-trip through the URL syntax.
    pub fn url(&self) -> SecretString {
        SecretString::from(format!(
            "postgres://{}:{}@{}:{}/{}",
            utf8_percent_encode(&self.username, URL_ENCODE_SET),
            utf8_percent_encode(self.password.expose_secret(), URL_ENCODE_SET),
            self.host,
            self.port,
            utf8_percent_encode(&self.database, URL_ENCODE_SET),
        ))
    }
}

/// Root of the configuration tree.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// Application name (`APP_NAME`), defaulting to the crate name of the
    /// running binary.
    pub name: String,
    /// Application version (`APP_VERSION`), defaulting to the crate
    /// version of the running binary — maintained by release-please.
    pub version: String,
    /// Enables verbose telemetry (`APP_DEBUG=true` -> DEBUG level,
    /// otherwise INFO).
    pub debug: bool,
    /// HTTP server settings.
    pub server: Server,
    /// PostgreSQL settings.
    pub database: Postgresql,
}

impl Config {
    /// Loads the configuration from every source.
    ///
    /// `name` and `version` seed the identity defaults and must come from
    /// the **binary** crate — `Config::load(env!("CARGO_PKG_NAME"),
    /// env!("CARGO_PKG_VERSION"))` — because those macros expand at compile
    /// time in the calling crate: evaluated here they would describe
    /// `common`, not the executable.
    ///
    /// Also loads `.env` beforehand so `APP_*` variables declared there are
    /// visible; a missing `.env` or `config.json` is not an error.
    ///
    /// # Errors
    ///
    /// Fails if a source cannot be parsed or if a value cannot be
    /// deserialized into the target type.
    pub fn load(name: &str, version: &str) -> anyhow::Result<Self> {
        // Populate the process environment from `.env` before the builder
        // reads it; `.ok()` because a missing file is a normal setup, not
        // an error.
        dotenvy::dotenv().ok();

        // Sources are registered from the least to the most specific: each
        // `add_source` overrides the previous layer key by key.
        let config = config::Config::builder()
            // Layer 1 — defaults: every key gets a value so the application
            // boots with zero external setup and deserialization never
            // fails on a missing field.
            .set_default("name", name)?
            .set_default("version", version)?
            .set_default("debug", false)?
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("database.host", "localhost")?
            .set_default("database.port", 5432)?
            .set_default("database.username", "root")?
            .set_default("database.password", "root")?
            .set_default("database.database", "app")?
            .set_default("database.pool.max_connections", 100)?
            .set_default("database.pool.min_connections", 5)?
            .set_default("database.pool.connect_timeout", 8)?
            .set_default("database.pool.acquire_timeout", 8)?
            // Idle and lifetime run on a different scale than the two
            // timeouts above: seconds here would recycle every connection
            // permanently under load.
            .set_default("database.pool.idle_timeout", 600)?
            .set_default("database.pool.max_lifetime", 1800)?
            // Layer 2 — optional system file, at the FHS-compliant path
            // for the Debian 13 / Docker deployment target: `/etc/<app>/`
            // is where a packaged service reads its configuration.
            .add_source(File::new("/etc/starterhub-rust-stack/app-config", FileFormat::Json).required(false))
            // Layer 3 — optional `app-config.json` in the working
            // directory: the local development override, JSON only by
            // design and never committed; it wins over the system file.
            .add_source(File::new("app-config", FileFormat::Json).required(false))
            // Layer 4 — `APP_*` environment variables, the runtime override
            // channel (containers, CI): `__` maps to nesting and
            // `try_parsing` coerces strings into the target numeric/bool
            // types.
            .add_source(
                Environment::with_prefix("APP")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        // Collapse the merged layers into the typed tree: from here on the
        // rest of the codebase only ever sees strongly-typed settings.
        Ok(config.try_deserialize()?)
    }
}
