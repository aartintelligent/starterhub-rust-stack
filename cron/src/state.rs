//! Shared cron state.

use sea_orm::DatabaseConnection;

/// State handed to every job when it is built.
///
/// Cloning is cheap: [`DatabaseConnection`] is a handle over a shared pool.
/// Extend this struct with any dependency the jobs need (configuration,
/// external clients, ...), exactly like the API's `AppState`.
#[derive(Clone)]
pub struct AppState {
    /// Handle to the PostgreSQL connection pool.
    pub conn: DatabaseConnection,
}

impl AppState {
    /// Builds the state from its dependencies.
    pub fn new(conn: DatabaseConnection) -> Self {
        Self { conn }
    }
}
