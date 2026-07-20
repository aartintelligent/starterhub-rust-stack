//! Unified error handling for cron jobs.
//!
//! Mirrors the philosophy of the API's `error.rs`: every job body returns
//! [`JobResult`], and every failure path converges to [`JobError`].
//! Infrastructure errors ([`DbErr`], [`anyhow::Error`]) convert
//! automatically via `#[from]`, so jobs can simply use the `?` operator.

use sea_orm::DbErr;
use thiserror::Error;

/// Convenience alias used as the return type of every job body.
pub type JobResult<T = ()> = Result<T, JobError>;

/// All the ways a cron job can fail.
///
/// A failing job is logged and waits for its next tick — it must never
/// take the cron engine down. Add one variant per new failure domain
/// (external API, filesystem, ...) instead of stringly-typed errors.
#[derive(Debug, Error)]
pub enum JobError {
    /// A database operation failed. Converted from sea-orm errors.
    #[error(transparent)]
    Database(#[from] DbErr),

    /// Any other unexpected failure.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}
