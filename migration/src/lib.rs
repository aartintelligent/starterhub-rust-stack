//! Database schema migrations.
//!
//! Migration files live in the [`source`] module, one module per schema
//! change, and are registered in [`Migrator::migrations`] in chronological
//! order. They are applied automatically at application boot and can also
//! be driven manually through the CLI in `main.rs`
//! (`cargo run -p migration -- <command>`).

pub use sea_orm_migration::prelude::*;

pub mod source;

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
