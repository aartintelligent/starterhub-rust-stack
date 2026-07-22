//! Cron jobs, one module per job, all built on the [`Job`] trait.
//!
//! Conventions:
//!
//! - Each job is a unit struct in its own file implementing [`Job`]:
//!   a stable `name` for telemetry, a hard-coded `schedule` (7-field cron
//!   with seconds: `sec min hour day-of-month month day-of-week year`) and
//!   an async `run` body returning [`JobResult`].
//! - Schedules are hard-coded on purpose: a cron expression is part of the
//!   job's behavior and belongs to code review, not to runtime
//!   configuration.
//! - Every new job is added to [`roster`], the single list the engine
//!   loads at boot; the cron wiring in [`register`] is generic and never
//!   changes.
//! - Schedules run in **UTC**, with **`dom AND dow`** semantics: when
//!   both day fields are restricted, a tick fires only when *both*
//!   match — the two departures from classic cron worth knowing when
//!   reviewing a schedule.
//! - A job declares its [`Overlap`] policy; by default a tick is skipped
//!   while the previous run is still in flight, so a slow job never
//!   overlaps itself.
//! - Every run is bounded by [`Job::timeout`]: a stuck run is
//!   cancelled and logged, freeing its overlap slot instead of wedging
//!   the job silently forever.
//! - Failures are logged uniformly by [`execute`] and wait for the next
//!   tick: no job can crash the cron engine.

mod hello;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio_cron_scheduler::{Job as CronJob, JobScheduler, JobSchedulerError};
use tokio_util::task::TaskTracker;

use crate::error::JobResult;
use crate::state::AppState;

/// Behavior when a tick fires while a previous run of the same job is
/// still in flight.
///
/// The guard is per-process: it protects a job from overlapping itself
/// inside one instance of the binary. Running several instances would
/// need a distributed lock (e.g. a PostgreSQL advisory lock) — out of
/// scope while the stack deploys as a single instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlap {
    /// Executions are independent; concurrent runs are fine.
    Allow,
    /// Skip the tick entirely; the run in flight keeps the slot.
    Skip,
}

/// Contract every cron job implements.
///
/// Object-safe on purpose (via `async_trait`): the roster manipulates
/// jobs as `dyn Job`, so adding one never touches the wiring.
#[async_trait::async_trait]
pub trait Job: Send + Sync + 'static {
    /// Stable identifier used in every log line about this job.
    fn name(&self) -> &'static str;

    /// Hard-coded cron expression driving the executions.
    fn schedule(&self) -> &'static str;

    /// Concurrency policy enforced by the engine wiring. Skipping is
    /// the safe default for jobs touching shared state: running
    /// concurrently with oneself is the behavior a job must opt into.
    fn overlap(&self) -> Overlap {
        Overlap::Skip
    }

    /// Upper bound on a single run, enforced by the engine wiring.
    ///
    /// Five minutes by default — generous for jobs meant to stay "short
    /// or idempotent", small enough that a run stuck on a dead
    /// dependency frees its overlap slot the same day it hung instead
    /// of silently stopping the job forever. Override per job when a
    /// legitimate run needs longer.
    fn timeout(&self) -> Duration {
        Duration::from_secs(300)
    }

    /// Body of the job; failures convert into [`crate::error::JobError`]
    /// with the `?` operator.
    async fn run(&self, state: &AppState) -> JobResult;
}

/// The full list of jobs the engine runs. Add every new job here.
fn roster() -> Vec<Arc<dyn Job>> {
    vec![Arc::new(hello::Hello)]
}

/// Registers the whole [`roster`] on the engine, tracking every run in
/// `tracker` so the server can drain in-flight jobs at shutdown.
///
/// # Errors
///
/// Fails if a job cannot be built (invalid schedule), cannot be added,
/// or shares its name with another roster entry — the name is the only
/// identity a job exposes to telemetry, so a duplicate would make every
/// log line ambiguous.
pub(crate) async fn register(
    scheduler: &JobScheduler,
    state: AppState,
    tracker: &TaskTracker,
) -> anyhow::Result<()> {
    let mut names = HashSet::new();

    for job in roster() {
        anyhow::ensure!(
            names.insert(job.name()),
            "duplicate job name in the roster: {}",
            job.name()
        );

        scheduler
            .add(into_cron_job(job, state.clone(), tracker.clone())?)
            .await?;
    }

    Ok(())
}

/// Adapts a [`Job`] into a `tokio-cron-scheduler` job.
///
/// This is the single bridge between our trait and the cron engine: the
/// pinned-box closure shape imposed by the crate lives here and nowhere
/// else. The engine's own job type is imported as `CronJob` to keep the
/// two concepts distinct.
fn into_cron_job(
    job: Arc<dyn Job>,
    state: AppState,
    tracker: TaskTracker,
) -> Result<CronJob, JobSchedulerError> {
    // One gate per job, shared by every tick of this job: this is what
    // makes the overlap policy per-job rather than global.
    let gate = Arc::new(Semaphore::new(1));

    CronJob::new_async(job.schedule(), move |_id, _scheduler| {
        let job = Arc::clone(&job);
        let state = state.clone();
        let gate = Arc::clone(&gate);

        // Tracked, because the scheduler spawns each tick as a detached
        // task it never awaits: the tracker is the server's only handle
        // on in-flight runs when draining at shutdown.
        Box::pin(tracker.track_future(async move { dispatch(job.as_ref(), &state, &gate).await }))
    })
}

/// Applies the job's [`Overlap`] policy, then runs it through [`execute`].
///
/// The semaphore permit is held across the whole run and released by
/// drop — even on failure — so a skipped slot can never leak: the next
/// tick after a completed run always executes.
async fn dispatch(job: &dyn Job, state: &AppState, gate: &Semaphore) {
    match job.overlap() {
        Overlap::Allow => execute(job, state).await,
        Overlap::Skip => match gate.try_acquire() {
            Ok(_permit) => execute(job, state).await,
            // Skipping is deliberate back-pressure, but a job skipped
            // often is too slow for its schedule: WARN makes it visible.
            Err(_) => tracing::warn!(
                job = job.name(),
                "job skipped: previous run still in flight"
            ),
        },
    }
}

/// Runs a job body, bounded by its [`Job::timeout`], and converts the
/// outcome into uniform telemetry.
///
/// Centralizing this guarantees a failing job is logged — never
/// propagated, since an error escaping the closure would compromise the
/// whole cron engine. The budget guards the engine, not the job: a run
/// stuck on a dead dependency must be cancelled (freeing its overlap
/// slot) and surface in telemetry, instead of wedging the job silently
/// forever.
async fn execute(job: &dyn Job, state: &AppState) {
    match tokio::time::timeout(job.timeout(), job.run(state)).await {
        Ok(Ok(())) => tracing::debug!(job = job.name(), "job completed"),
        // Alternate format on purpose: the whole anyhow context chain,
        // because this log line is the only artifact a failed run
        // leaves behind.
        Ok(Err(error)) => {
            tracing::error!(job = job.name(), error = %format_args!("{error:#}"), "job failed");
        }
        Err(_) => tracing::error!(
            job = job.name(),
            budget = ?job.timeout(),
            "job timed out and was cancelled"
        ),
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests of the private job wiring: the reference `hello` job,
    //! the outcome telemetry of [`execute`], the overlap policy in
    //! [`dispatch`] and the schedule validation in [`into_cron_job`].
    //! The happy registration path is covered end to end by the engine
    //! lifecycle integration test.

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use sea_orm::{DatabaseBackend, MockDatabase};
    use tokio::sync::Notify;

    use super::*;
    use crate::error::JobError;

    /// Job failing on purpose, on an invalid schedule on purpose.
    struct Failing;

    #[async_trait::async_trait]
    impl Job for Failing {
        fn name(&self) -> &'static str {
            "failing"
        }

        fn schedule(&self) -> &'static str {
            "not a cron expression"
        }

        async fn run(&self, _state: &AppState) -> JobResult {
            Err(JobError::Internal(anyhow::anyhow!("boom")))
        }
    }

    /// Mock-backed state handed to jobs under test.
    fn state() -> AppState {
        // Global TRACE-level test subscriber (first caller wins), so the
        // outcome log statements of `execute` are fully evaluated.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();

        AppState::new(MockDatabase::new(DatabaseBackend::Postgres).into_connection())
    }

    /// The reference job declares its identity and runs successfully.
    #[tokio::test]
    async fn hello_declares_itself_and_runs() {
        let job = hello::Hello;

        assert_eq!(job.name(), "hello");
        assert_eq!(job.schedule(), "0 * * * * * *");
        assert_eq!(job.overlap(), Overlap::Skip);
        assert_eq!(job.timeout(), Duration::from_secs(300));
        assert!(job.run(&state()).await.is_ok());
    }

    /// Both outcomes convert into telemetry; a failure never propagates
    /// out of [`execute`], so no job can take the engine down.
    #[tokio::test]
    async fn execute_never_propagates_the_outcome() {
        execute(&hello::Hello, &state()).await;
        execute(&Failing, &state()).await;
    }

    /// An invalid hard-coded schedule is rejected at adaptation time —
    /// this is what aborts the boot instead of failing silently later.
    #[test]
    fn invalid_schedule_is_rejected() {
        assert_eq!(Failing.name(), "failing");
        assert!(into_cron_job(Arc::new(Failing), state(), TaskTracker::new()).is_err());
    }

    /// Job hanging far past its deliberately tiny budget.
    struct Hung;

    #[async_trait::async_trait]
    impl Job for Hung {
        fn name(&self) -> &'static str {
            "hung"
        }

        fn schedule(&self) -> &'static str {
            "* * * * * * *"
        }

        /// Tiny on purpose: the test proves the budget cuts the run.
        fn timeout(&self) -> Duration {
            Duration::from_millis(20)
        }

        async fn run(&self, _state: &AppState) -> JobResult {
            tokio::time::sleep(Duration::from_secs(3600)).await;

            Ok(())
        }
    }

    /// A run exceeding its budget is cancelled: `execute` returns
    /// instead of inheriting the hang.
    #[tokio::test]
    async fn hung_job_is_cancelled_by_its_budget() {
        // The outer bound only makes a regression fail fast instead of
        // hanging the suite; the job's own budget is what must fire.
        tokio::time::timeout(Duration::from_secs(5), execute(&Hung, &state()))
            .await
            .expect("the job budget must cut the run");
    }

    /// A cancelled run frees its overlap slot, so the next tick executes
    /// instead of being skipped forever — the failure mode a timeout-less
    /// Skip policy would create.
    #[tokio::test]
    async fn timed_out_run_frees_its_overlap_slot() {
        let state = state();
        let gate = Semaphore::new(1);

        dispatch(&Hung, &state, &gate).await;

        assert_eq!(gate.available_permits(), 1);
    }

    /// Job that parks inside `run` until released, counting executions:
    /// lets a test hold a run in flight while it fires more ticks.
    struct Parked {
        /// Overlap policy under test.
        overlap: Overlap,
        /// Signalled each time a run starts.
        started: Arc<Notify>,
        /// Awaited by each run before returning.
        release: Arc<Notify>,
        /// Number of runs that actually started.
        runs: AtomicUsize,
    }

    impl Parked {
        fn new(overlap: Overlap) -> Self {
            Self {
                overlap,
                started: Arc::new(Notify::new()),
                release: Arc::new(Notify::new()),
                runs: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait::async_trait]
    impl Job for Parked {
        fn name(&self) -> &'static str {
            "parked"
        }

        fn schedule(&self) -> &'static str {
            "* * * * * * *"
        }

        fn overlap(&self) -> Overlap {
            self.overlap
        }

        async fn run(&self, _state: &AppState) -> JobResult {
            self.runs.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            self.release.notified().await;

            Ok(())
        }
    }

    /// Fires one tick of `job` on its own task, as the engine would.
    fn tick(
        job: &Arc<Parked>,
        state: &AppState,
        gate: &Arc<Semaphore>,
    ) -> tokio::task::JoinHandle<()> {
        let job = Arc::clone(job);
        let state = state.clone();
        let gate = Arc::clone(gate);

        tokio::spawn(async move { dispatch(job.as_ref(), &state, &gate).await })
    }

    /// Under [`Overlap::Skip`], a tick firing while the previous run is
    /// in flight is dropped — and the slot frees up once the run ends.
    #[tokio::test]
    async fn skip_policy_drops_the_overlapping_tick() {
        let job = Arc::new(Parked::new(Overlap::Skip));
        let state = state();
        let gate = Arc::new(Semaphore::new(1));

        // First tick: takes the slot and parks inside `run`.
        let first = tick(&job, &state, &gate);
        job.started.notified().await;

        // Second tick while the first is in flight: skipped, so it
        // returns immediately without waiting for any release.
        dispatch(job.as_ref(), &state, &gate).await;
        assert_eq!(job.runs.load(Ordering::SeqCst), 1);

        // Let the first run finish: its permit returns by drop.
        job.release.notify_one();
        first.await.expect("the first tick must complete");

        // The slot is free again: the next tick executes normally.
        job.release.notify_one();
        dispatch(job.as_ref(), &state, &gate).await;
        assert_eq!(job.runs.load(Ordering::SeqCst), 2);
    }

    /// Under [`Overlap::Allow`], ticks run concurrently: the second one
    /// starts while the first is still parked in flight.
    #[tokio::test]
    async fn allow_policy_lets_ticks_overlap() {
        let job = Arc::new(Parked::new(Overlap::Allow));
        let state = state();
        let gate = Arc::new(Semaphore::new(1));

        let first = tick(&job, &state, &gate);
        job.started.notified().await;
        let second = tick(&job, &state, &gate);
        job.started.notified().await;
        assert_eq!(job.runs.load(Ordering::SeqCst), 2);

        job.release.notify_one();
        job.release.notify_one();
        first.await.expect("the first tick must complete");
        second.await.expect("the second tick must complete");
    }

    /// A failing run releases the slot on the way out: the guard can
    /// never leak a permit and wedge the job forever.
    #[tokio::test]
    async fn skip_policy_frees_the_slot_after_a_failure() {
        let state = state();
        let gate = Semaphore::new(1);

        dispatch(&Failing, &state, &gate).await;
        dispatch(&Failing, &state, &gate).await;

        assert_eq!(gate.available_permits(), 1);
    }

    /// Job on the fastest possible schedule, signalling each execution.
    struct Ticking(Arc<Notify>);

    #[async_trait::async_trait]
    impl Job for Ticking {
        fn name(&self) -> &'static str {
            "ticking"
        }

        /// Every second: the shortest wait a real engine tick allows.
        fn schedule(&self) -> &'static str {
            "* * * * * * *"
        }

        async fn run(&self, _state: &AppState) -> JobResult {
            self.0.notify_one();

            Ok(())
        }
    }

    /// A registered job fires through the adapter closure end to end: the
    /// engine ticks, the closure clones its captures and runs the body.
    #[tokio::test]
    async fn registered_job_fires_through_the_adapter() {
        let fired = Arc::new(Notify::new());
        let job = into_cron_job(
            Arc::new(Ticking(Arc::clone(&fired))),
            state(),
            TaskTracker::new(),
        )
        .expect("the schedule is valid");

        let mut scheduler = JobScheduler::new().await.expect("engine must build");
        scheduler.add(job).await.expect("job must register");
        scheduler.start().await.expect("engine must start");

        // The schedule fires every second: five is a generous bound that
        // keeps the test deterministic on a loaded CI runner.
        tokio::time::timeout(Duration::from_secs(5), fired.notified())
            .await
            .expect("the job must fire within its schedule period");

        scheduler.shutdown().await.expect("engine must stop");
    }

    /// The tracker follows a run across the whole engine path, and the
    /// drain (close + wait) blocks until that run completes — the
    /// mechanism `Server::run` relies on for graceful shutdown.
    #[tokio::test]
    async fn drain_waits_for_the_inflight_run() {
        let job = Arc::new(Parked::new(Overlap::Skip));
        let tracker = TaskTracker::new();
        let cron_job = into_cron_job(Arc::clone(&job) as Arc<dyn Job>, state(), tracker.clone())
            .expect("the schedule is valid");

        let mut scheduler = JobScheduler::new().await.expect("engine must build");
        scheduler.add(cron_job).await.expect("job must register");
        scheduler.start().await.expect("engine must start");

        // Wait until a run is parked in flight, then stop the ticks.
        tokio::time::timeout(Duration::from_secs(5), job.started.notified())
            .await
            .expect("a run must start within its schedule period");
        scheduler.shutdown().await.expect("engine must stop");
        tracker.close();

        // The drain must NOT resolve while the run is parked...
        assert!(
            tokio::time::timeout(Duration::from_millis(100), tracker.wait())
                .await
                .is_err(),
            "the drain must wait for the in-flight run"
        );

        // ...and must resolve promptly once it completes.
        job.release.notify_one();
        tokio::time::timeout(Duration::from_secs(5), tracker.wait())
            .await
            .expect("the drain must finish once the run completes");
    }
}
