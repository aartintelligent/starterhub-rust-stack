//! Strongly-typed, layered application configuration.
//!
//! Sources are merged in ascending priority:
//!
//! 1. Hard-coded defaults (the application boots with zero external setup).
//! 2. An optional system file `/etc/ipam/app-config.json` — the FHS
//!    location for a Debian 13 deployment, mounted or baked into the
//!    Docker image.
//! 3. An optional `app-config.json` in the working directory, the local
//!    development override (never committed to the repository).
//! 4. Environment variables prefixed with `APP_`, using `__` as the nesting
//!    separator, e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS=50`.

use config::{Environment, File, FileFormat};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

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
    pub fn url(&self) -> SecretString {
        SecretString::from(format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username,
            self.password.expose_secret(),
            self.host,
            self.port,
            self.database
        ))
    }
}

/// Root of the configuration tree.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
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
    /// Also loads `.env` beforehand so `APP_*` variables declared there are
    /// visible; a missing `.env` or `config.json` is not an error.
    ///
    /// # Errors
    ///
    /// Fails if a source cannot be parsed or if a value cannot be
    /// deserialized into the target type.
    pub fn load() -> anyhow::Result<Self> {
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
            .set_default("debug", false)?
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("database.host", "localhost")?
            .set_default("database.port", 5432)?
            .set_default("database.username", "root")?
            .set_default("database.password", "root")?
            .set_default("database.database", "ipam")?
            .set_default("database.pool.max_connections", 100)?
            .set_default("database.pool.min_connections", 5)?
            .set_default("database.pool.connect_timeout", 8)?
            .set_default("database.pool.acquire_timeout", 8)?
            .set_default("database.pool.idle_timeout", 8)?
            .set_default("database.pool.max_lifetime", 8)?
            // Layer 2 — optional system file, at the FHS-compliant path
            // for the Debian 13 / Docker deployment target: `/etc/<app>/`
            // is where a packaged service reads its configuration.
            .add_source(File::new("/etc/ipam/app-config", FileFormat::Json).required(false))
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
