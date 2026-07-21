//! Cron bootstrap.
//!
//! Mirrors the API's `server.rs`: decouples "how the cron engine starts"
//! from "what the application boots". The binary crate provides the state
//! and the shutdown future; this module owns the engine lifecycle —
//! build, register the job roster, tick, stop cleanly.

use std::future::Future;

use anyhow::Context;
use tokio_cron_scheduler::JobScheduler;

use crate::job;
use crate::state::AppState;

/// A configured cron engine with every job registered, ready to tick.
pub struct Server {
    /// Underlying tokio-cron-scheduler engine.
    inner: JobScheduler,
}

impl Server {
    /// Builds the engine and registers the full job roster, each job
    /// receiving a clone of `state`.
    ///
    /// # Errors
    ///
    /// Fails if the engine cannot be created or a job refuses to
    /// register (typically an invalid hard-coded schedule); every error
    /// carries enough context to be actionable from the logs.
    pub async fn new(state: AppState) -> anyhow::Result<Self> {
        // Create the bare engine first: it must exist before any job can
        // attach, and a failure here means the runtime itself is unusable.
        let inner = JobScheduler::new()
            .await
            .context("failed to create the cron engine")?;

        // Register the whole roster at construction time, not at start:
        // an invalid hard-coded schedule aborts the boot, instead of
        // surfacing after the application already reports itself healthy.
        job::register(&inner, state)
            .await
            .context("failed to register the job roster")?;

        Ok(Self { inner })
    }

    /// Starts ticking, then waits for `shutdown` and stops cleanly.
    ///
    /// A clean stop halts the tick loop so no new job fires during the
    /// shutdown window. Jobs already in flight run as detached tasks the
    /// scheduler does not await: a long-running job can still be cut by
    /// the process exit, so keep job bodies short or idempotent.
    ///
    /// # Errors
    ///
    /// Fails if the engine cannot start or cannot shut down.
    pub async fn run(mut self, shutdown: impl Future<Output = ()> + Send) -> anyhow::Result<()> {
        // Start the tick loop; from this point jobs fire on their
        // schedules, so this belongs after migrations and pool creation
        // in the boot order decided by the binary crate.
        self.inner
            .start()
            .await
            .context("failed to start the cron engine")?;

        // One structured line to confirm the engine is ticking — the
        // operator's cue that jobs will fire, symmetric with the API's
        // "listening on" line.
        tracing::info!("cron engine started");

        // Park here until the process-wide shutdown fires: the engine
        // works in background tasks, this future's only job is to keep
        // ownership alive and time the stop.
        shutdown.await;

        // Announce before stopping: if the shutdown below hangs, the logs
        // show exactly which phase we died in.
        tracing::info!("cron engine shutting down");

        // Explicit engine shutdown rather than dropping it: the tick
        // loop stops cleanly and no new job fires. In-flight jobs are
        // detached tasks the scheduler does not await — the process exit
        // may still cut one, which is why job bodies must stay short or
        // idempotent.
        self.inner
            .shutdown()
            .await
            .context("failed to stop the cron engine")?;

        // Terminal line for a clean stop, so operators can tell a
        // graceful exit from a crash in the journal.
        tracing::info!("cron engine stopped");

        Ok(())
    }
}
