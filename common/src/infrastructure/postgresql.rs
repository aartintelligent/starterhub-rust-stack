//! PostgreSQL connectivity via sea-orm.

use std::time::Duration;

use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};
use secrecy::ExposeSecret;

use crate::config::Postgresql;

/// Opens a connection pool configured from [`Postgresql`].
///
/// Every pool knob (size, timeouts, lifetime) comes from `config.pool`, so
/// behavior is tunable per environment without touching the code.
///
/// # Errors
///
/// Fails if the server is unreachable or the credentials are rejected.
pub async fn connect(config: &Postgresql) -> Result<DatabaseConnection, DbErr> {
    // Start from the assembled URL so credentials and target live in one
    // place: the typed configuration, never a handwritten string. This is
    // the single point where the secret is exposed — the connection
    // builder is its final consumer.
    let mut opt = ConnectOptions::new(config.url().expose_secret());

    // Apply every pool knob explicitly rather than trusting SQLX defaults:
    // production tuning then happens in configuration, not in code.
    opt.max_connections(config.pool.max_connections)
        .min_connections(config.pool.min_connections)
        .connect_timeout(Duration::from_secs(config.pool.connect_timeout))
        .acquire_timeout(Duration::from_secs(config.pool.acquire_timeout))
        .idle_timeout(Duration::from_secs(config.pool.idle_timeout))
        .max_lifetime(Duration::from_secs(config.pool.max_lifetime));

    // Establish the pool; sea-orm validates connectivity here, so a broken
    // target surfaces at boot instead of on the first query.
    Database::connect(opt).await
}
