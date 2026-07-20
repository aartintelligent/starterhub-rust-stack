//! Standalone migration CLI.
//!
//! Wraps the sea-orm-migration command line (`up`, `down`, `status`,
//! `generate`, ...): run `cargo run -p migration -- <command>`.
//!
//! The target database is resolved from the shared configuration of the
//! `common` crate, so the CLI honours the exact same variables as the API
//! (`APP_DATABASE__HOST`, `APP_DATABASE__PORT`, ...).

use common::config::Config;
use common::infrastructure::postgresql;
use sea_orm_migration::prelude::*;

/// Boots the migration CLI on top of the shared configuration.
#[tokio::main]
async fn main() {
    // Resolve the configuration up front: the CLI is useless without a
    // database target, so a malformed source must abort immediately.
    let config = Config::load().expect("failed to load configuration");

    // Delegate to the sea-orm-migration CLI, but keep control of the
    // connection: injecting our own factory guarantees the CLI reaches the
    // exact same database, with the same pool settings, as the API —
    // instead of resolving a separate `DATABASE_URL` on its own.
    cli::run_cli_with_custom_connection(migration::Migrator, async || {
        postgresql::connect(&config.database).await
    })
    .await;
}
