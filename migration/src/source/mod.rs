//! Migration files, one module per schema change.
//!
//! Naming follows the sea-orm convention `mYYYYMMDD_NNNNNN_<label>` so the
//! chronological order is obvious from the file name alone. Declare each
//! migration here, then register it in [`crate::Migrator::migrations`]:
//!
//! ```text
//! pub mod m20260720_000001_create_subnet_table;
//! ```
//!
//! Note: `cargo run -p migration -- generate <label>` scaffolds the file in
//! `src/` and registers it in `lib.rs`; move the file here and fix the two
//! references, or create it by hand directly in this folder.
