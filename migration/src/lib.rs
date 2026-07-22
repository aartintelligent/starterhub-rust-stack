//! Database schema migrations.
//!
//! Migration files live in the [`source`] module, one module per schema
//! change, and are registered in [`Migrator::migrations`] in chronological
//! order. They are applied automatically at application boot and can also
//! be driven manually through the CLI in `main.rs`
//! (`cargo run -p migration -- <command>`).

// Through sea-orm-migration's re-export: the crate needs no direct
// sea-orm dependency of its own (`ConnectionTrait` already arrives via
// the prelude below).
pub use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{DatabaseConnection, TransactionTrait};

pub mod source;

/// Key of the PostgreSQL advisory lock serializing migration runs.
///
/// Any stable value works — advisory keys are just a 64-bit namespace
/// shared by consenting clients; this one was chosen at random once and
/// must never change, or two builds of the application would stop
/// excluding each other.
const MIGRATION_LOCK_KEY: i64 = 0x5f8a_11ab_c0de_0001;

/// Applies every pending migration under an advisory lock.
///
/// `sea-orm-migration` takes no lock of its own, so two replicas booting
/// simultaneously with a pending migration would both run it: one wins,
/// the other crash-loops on "duplicate table" until its restart lands
/// after the winner. The lock serializes them instead. It is
/// transaction-scoped (`pg_advisory_xact_lock`): acquired and released
/// on the same connection by construction, and impossible to leak — the
/// commit or any failure releases it.
///
/// # Errors
///
/// Fails if the transaction, the lock acquisition or a migration fails;
/// the migration error is the interesting one and is returned as-is.
pub async fn up_guarded(conn: &DatabaseConnection) -> Result<(), DbErr> {
    let txn = conn.begin().await?;

    // Blocks until the concurrent migrator (if any) commits: by then the
    // migrations table is up to date and our own run is a no-op.
    txn.execute_unprepared(&format!(
        "SELECT pg_advisory_xact_lock({MIGRATION_LOCK_KEY})"
    ))
    .await?;

    Migrator::up(&txn, None).await?;

    txn.commit().await
}

/// Aggregates every migration of the application.
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// Full, ordered list of migrations. Append new ones at the end,
    /// never reorder or delete an already-shipped entry, e.g.:
    ///
    /// ```text
    /// vec![Box::new(source::m20260720_000001_create_subnet_table::Migration)]
    /// ```
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![]
    }
}
