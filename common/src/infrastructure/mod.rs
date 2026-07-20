//! Helpers around external systems.
//!
//! One module per system (database, cache, message broker, ...), each
//! consuming its own section of the configuration tree.

pub mod postgresql;
