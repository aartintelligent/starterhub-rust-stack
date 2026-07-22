//! Strongly-typed, layered application configuration.
//!
//! Sources are merged in ascending priority:
//!
//! 1. Hard-coded defaults (the application boots with zero external setup).
//! 2. An optional system file `/etc/<name>/app-config.json` — the FHS
//!    location for a Debian 13 deployment, mounted or baked into the
//!    Docker image; `<name>` is the application name the binary passes
//!    to [`Config::load`], so a renamed binary reads its own directory.
//! 3. An optional `app-config.json` in the working directory, the local
//!    development override (never committed to the repository).
//! 4. Environment variables prefixed with `APP_`, using `__` as the nesting
//!    separator, e.g. `APP_DATABASE__POOL__MAX_CONNECTIONS=50`.
//!
//! The tree is strict end to end: an unknown key aborts the load (the
//! `APP_*` namespace belongs to the application, so a typoed override
//! must never be silently ignored), and cross-field invariants are
//! enforced by [`Config::validate`] before the configuration reaches
//! any consumer.

// The `config` crate's `Environment` (the env-vars source) collides with
// the local `Environment` enum below: keep the import list free of it and
// use the fully-qualified path at the single point of use.
use config::{File, FileFormat};
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

/// Loads `./.env` into the process environment, if present.
///
/// Called by the binaries before [`Config::load`], never by the library
/// itself: loading environment files is an executable's concern, and
/// keeping it out of `Config::load` keeps the library pure and its
/// tests hermetic. Explicitly the working directory's file — dotenvy's
/// default lookup walks ancestor directories, where a `.env` belonging
/// to another project would silently win.
///
/// # Errors
///
/// A missing file is a normal setup and returns `Ok`; a present but
/// malformed file aborts, because a file someone took the trouble to
/// write must never be silently half-read.
pub fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::from_path(".env") {
        Ok(()) => Ok(()),
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow::Error::from(error).context("failed to load .env")),
    }
}

/// Deserializes a scalar arriving either in its native type (defaults,
/// JSON file layers) or as a string (environment variables deliver
/// everything as text).
///
/// Applied field by field to the numeric and boolean settings only, on
/// purpose: blanket source-level coercion (`try_parsing`) round-trips
/// *string-typed* values through numbers and silently corrupts them — a
/// password of `0123` must stay `"0123"`, never become `"123"`.
fn native_or_string<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de> + std::str::FromStr,
    T::Err: std::fmt::Display,
{
    /// The two shapes a typed scalar can arrive in.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Raw<T> {
        /// Already typed by the source.
        Native(T),
        /// Textual, to be parsed into the target type.
        Text(String),
    }

    match Raw::<T>::deserialize(deserializer)? {
        Raw::Native(value) => Ok(value),
        Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
    }
}

/// Deployment environment of the running instance (`environment`,
/// `APP_ENVIRONMENT`).
///
/// Drives environment-dependent behavior — e.g. the interactive API
/// documentation is only exposed in [`Environment::Local`] and
/// [`Environment::Development`].
///
/// Strict on purpose: exactly `local`, `development`, `staging` or
/// `production`, nothing else — no short forms, no case variants. A
/// typo in this value silently flips security-relevant behavior (docs
/// exposure today, more tomorrow), so it must abort the boot instead
/// of deserializing into a guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Developer workstation.
    Local,
    /// Shared development / integration deployment.
    Development,
    /// Pre-production deployment.
    Staging,
    /// Production deployment.
    Production,
}

impl Environment {
    /// True where the interactive API documentation (`/docs`) is exposed:
    /// exploration belongs to local and development environments, never
    /// to staging or production.
    pub fn exposes_docs(self) -> bool {
        matches!(self, Environment::Local | Environment::Development)
    }
}

/// HTTP server settings (`server.*`).
///
/// `deny_unknown_fields` on every struct of the tree: a typoed key must
/// abort the boot, not silently fall back to a default.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    /// Interface to bind, e.g. `127.0.0.1` or `0.0.0.0`.
    pub host: String,
    /// TCP port to listen on.
    #[serde(deserialize_with = "native_or_string")]
    pub port: u16,
    /// Upper bound on the total processing time of one request, in
    /// seconds: past it the client receives a JSON timeout error
    /// instead of holding a connection open indefinitely.
    #[serde(deserialize_with = "native_or_string")]
    pub timeout: u64,
}

impl Server {
    /// Bindable address in `host:port` form.
    ///
    /// An IPv6 host (spotted by its colons) is bracketed, because a bare
    /// `:::8080` is unparseable next to the port separator.
    pub fn url(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// Connection pool tuning (`database.pool.*`). Durations are in seconds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostgresqlPool {
    /// Upper bound of open connections.
    #[serde(deserialize_with = "native_or_string")]
    pub max_connections: u32,
    /// Connections kept warm even when idle.
    #[serde(deserialize_with = "native_or_string")]
    pub min_connections: u32,
    /// Max time to wait when establishing a new connection.
    #[serde(deserialize_with = "native_or_string")]
    pub connect_timeout: u64,
    /// Max time to wait for a free connection from the pool.
    #[serde(deserialize_with = "native_or_string")]
    pub acquire_timeout: u64,
    /// Idle time after which a connection is closed.
    #[serde(deserialize_with = "native_or_string")]
    pub idle_timeout: u64,
    /// Max lifetime of a connection before it is recycled.
    #[serde(deserialize_with = "native_or_string")]
    pub max_lifetime: u64,
}

/// PostgreSQL settings (`database.*`), kept split field by field so each
/// value can be overridden independently.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Postgresql {
    /// Database server hostname.
    pub host: String,
    /// Database server port.
    #[serde(deserialize_with = "native_or_string")]
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
    /// `%`, ...) survive the round-trip through the URL syntax. The host
    /// is bracketed when IPv6 (a bare colon would read as the port
    /// separator) and percent-encoded otherwise — brackets themselves
    /// must not be encoded, so the two cases are exclusive.
    pub fn url(&self) -> SecretString {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            utf8_percent_encode(&self.host, URL_ENCODE_SET).to_string()
        };

        SecretString::from(format!(
            "postgres://{}:{}@{}:{}/{}",
            utf8_percent_encode(&self.username, URL_ENCODE_SET),
            utf8_percent_encode(self.password.expose_secret(), URL_ENCODE_SET),
            host,
            self.port,
            utf8_percent_encode(&self.database, URL_ENCODE_SET),
        ))
    }
}

/// Root of the configuration tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Application name (`APP_NAME`), defaulting to the crate name of the
    /// running binary.
    pub name: String,
    /// Application version (`APP_VERSION`), defaulting to the crate
    /// version of the running binary — maintained by release-please.
    pub version: String,
    /// Deployment environment (`APP_ENVIRONMENT`), driving
    /// environment-dependent behavior such as exposing `/docs`.
    pub environment: Environment,
    /// Enables verbose telemetry (`APP_DEBUG=true` -> DEBUG level,
    /// otherwise INFO).
    #[serde(deserialize_with = "native_or_string")]
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
    /// Reads the process environment as-is: the binaries call
    /// [`load_dotenv`] first when they want `./.env` applied.
    ///
    /// # Errors
    ///
    /// Fails if a source cannot be parsed, a value cannot be
    /// deserialized into the target type, a key is unknown, or a
    /// [`Config::validate`] invariant does not hold.
    pub fn load(name: &str, version: &str) -> anyhow::Result<Self> {
        // Sources are registered from the least to the most specific: each
        // `add_source` overrides the previous layer key by key.
        let config = config::Config::builder()
            // Layer 1 — defaults: every key gets a value so the application
            // boots with zero external setup and deserialization never
            // fails on a missing field.
            .set_default("name", name)?
            .set_default("version", version)?
            // `local` is the safe zero-setup default; the production
            // image overrides it (`APP_ENVIRONMENT=production` in the
            // Dockerfile).
            .set_default("environment", "local")?
            .set_default("debug", false)?
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            .set_default("server.timeout", 30)?
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
            // is where a packaged service reads its configuration. The
            // path derives from `name` so a renamed binary keeps a live
            // system layer instead of silently reading nothing.
            .add_source(File::new(&format!("/etc/{name}/app-config"), FileFormat::Json).required(false))
            // Layer 3 — optional `app-config.json` in the working
            // directory: the local development override, JSON only by
            // design and never committed; it wins over the system file.
            .add_source(File::new("app-config", FileFormat::Json).required(false))
            // Layer 4 — `APP_*` environment variables, the runtime override
            // channel (containers, CI): `__` maps to nesting. No
            // source-level `try_parsing`: it would round-trip string
            // fields through numbers ("0123" -> 123 -> "123"); the typed
            // fields parse their own text via `native_or_string` instead.
            .add_source(
                config::Environment::with_prefix("APP")
                    // Explicit on purpose: when only `separator` is set,
                    // the crate defaults the prefix separator to it too,
                    // and the whole documented `APP_*` convention would
                    // silently require `APP__*`.
                    .prefix_separator("_")
                    .separator("__"),
            )
            .build()?;

        // Collapse the merged layers into the typed tree: from here on the
        // rest of the codebase only ever sees strongly-typed settings —
        // and only after the cross-field invariants held.
        let config: Self = config.try_deserialize()?;
        config.validate()
    }

    /// Enforces the cross-field invariants deserialization cannot.
    ///
    /// # Errors
    ///
    /// Fails on a configuration that would boot into a broken or unsafe
    /// state; each message names the offending key and the remedy.
    fn validate(self) -> anyhow::Result<Self> {
        anyhow::ensure!(
            self.server.timeout > 0,
            "server.timeout must be at least 1 second: 0 would time out every request instantly"
        );
        anyhow::ensure!(
            self.database.pool.connect_timeout > 0 && self.database.pool.acquire_timeout > 0,
            "database.pool timeouts must be at least 1 second: 0 would fail every acquisition instantly"
        );
        anyhow::ensure!(
            self.database.pool.min_connections <= self.database.pool.max_connections,
            "database.pool.min_connections ({}) cannot exceed max_connections ({})",
            self.database.pool.min_connections,
            self.database.pool.max_connections,
        );
        // The template's out-of-the-box credentials must never reach
        // production: without this check, forgetting APP_DATABASE__PASSWORD
        // in a deployment silently authenticates as root/root.
        if self.environment == Environment::Production {
            anyhow::ensure!(
                self.database.password.expose_secret() != "root",
                "refusing to boot in production with the default database password; set APP_DATABASE__PASSWORD"
            );
        }

        Ok(self)
    }
}
