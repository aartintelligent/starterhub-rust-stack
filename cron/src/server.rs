//! Cron bootstrap.
//!
//! Mirrors the API's `server.rs`: decouples "how the cron engine starts"
//! from "what the application boots". The binary crate provides the state
//! and the shutdown future; this module owns the engine lifecycle —
//! build, register the job roster, tick, stop cleanly.

use std::future::Future;
use std::time::Duration;

use anyhow::Context;
use tokio_cron_scheduler::JobScheduler;
use tokio_util::task::TaskTracker;

use crate::job;
use crate::state::AppState;

/// Upper bound on the shutdown drain. Every run is already bounded by
/// its own [`crate::job::Job::timeout`]; this second fence only exists
/// so the process exit stays deterministic even if that invariant is
/// ever broken.
const DRAIN_GRACE: Duration = Duration::from_secs(30);

/// A configured cron engine with every job registered, ready to tick.
pub struct Server {
    /// Underlying tokio-cron-scheduler engine.
    inner: JobScheduler,
    /// Registry of in-flight job runs, drained at shutdown.
    tracker: TaskTracker,
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
        // The tracker follows every run, so shutdown can drain them.
        let tracker = TaskTracker::new();
        job::register(&inner, state, &tracker)
            .await
            .context("failed to register the job roster")?;

        Ok(Self { inner, tracker })
    }

    /// Starts ticking, then waits for `shutdown` and stops cleanly.
    ///
    /// A clean stop halts the tick loop so no new job fires during the
    /// shutdown window, then **drains** the runs already in flight: each
    /// is bounded by its own [`crate::job::Job::timeout`], and the drain
    /// itself is fenced by [`DRAIN_GRACE`] so the process exit stays
    /// deterministic no matter what.
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
        // loop stops cleanly and no new job fires. In-flight runs are
        // detached tasks the scheduler does not await — draining them is
        // the tracker's job, just below.
        self.inner
            .shutdown()
            .await
            .context("failed to stop the cron engine")?;

        // Drain the in-flight runs. The tick loop is stopped, so the
        // tracker can no longer grow; `close` flips it so `wait`
        // resolves once the last tracked run finishes. A run cut here
        // would be mid-write: this wait is what makes the "graceful" in
        // graceful shutdown true for jobs, not only for the API.
        self.tracker.close();
        if tokio::time::timeout(DRAIN_GRACE, self.tracker.wait())
            .await
            .is_err()
        {
            // Reaching this line means a job outlived its own budget —
            // a bug worth a loud trace, not a hung process.
            tracing::warn!(
                grace = ?DRAIN_GRACE,
                "jobs still in flight at the end of the drain grace; exiting anyway"
            );
        }

        // Terminal line for a clean stop, so operators can tell a
        // graceful exit from a crash in the journal.
        tracing::info!("cron engine stopped");

        Ok(())
    }
}
