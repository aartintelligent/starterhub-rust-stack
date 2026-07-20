//! Cron component of the application.
//!
//! The crate mirrors the layout of the `api` crate:
//!
//! - [`error`]: unified [`error::JobError`] type; job bodies return
//!   [`error::JobResult`].
//! - [`job`]: the jobs themselves, built on the [`job::Job`] trait — one
//!   module per job, hard-coded schedules, registered in a single roster.
//! - [`server`]: cron bootstrap ([`server::Server`]), driving the engine
//!   lifecycle from build to graceful stop.
//! - [`state`]: shared dependencies handed to every job.
//!
//! Built on `tokio-cron-scheduler`. The binary crate drives the lifecycle:
//! build with `Server::new`, then hand a shutdown future to `Server::run`.

pub mod error;
pub mod job;
pub mod server;
pub mod state;
