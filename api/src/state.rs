//! Shared application state.

use sea_orm::DatabaseConnection;

/// State injected into every handler via axum's `State` extractor.
///
/// Cloning is cheap: [`DatabaseConnection`] is a handle over a shared pool.
/// Extend this struct with any dependency the handlers need (configuration,
/// external clients, caches, ...).
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
