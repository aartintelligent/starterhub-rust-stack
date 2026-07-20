//! Shared building blocks used by every crate of the workspace.
//!
//! - [`config`]: strongly-typed, layered application configuration.
//! - [`infrastructure`]: helpers around external systems (database, ...).
//! - [`telemetry`]: tracing/logging initialization.

pub mod config;
pub mod infrastructure;
pub mod telemetry;
