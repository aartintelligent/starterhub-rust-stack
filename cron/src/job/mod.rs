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
//! - Failures are logged uniformly by [`execute`] and wait for the next
//!   tick: no job can crash the cron engine.

mod hello;

use std::sync::Arc;

use tokio_cron_scheduler::{Job as CronJob, JobScheduler, JobSchedulerError};

use crate::error::JobResult;
use crate::state::AppState;

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

    /// Body of the job; failures convert into [`crate::error::JobError`]
    /// with the `?` operator.
    async fn run(&self, state: &AppState) -> JobResult;
}

/// The full list of jobs the engine runs. Add every new job here.
fn roster() -> Vec<Arc<dyn Job>> {
    vec![Arc::new(hello::Hello)]
}

/// Registers the whole [`roster`] on the engine.
///
/// # Errors
///
/// Fails if a job cannot be built (invalid schedule) or added.
pub(crate) async fn register(scheduler: &JobScheduler, state: AppState) -> anyhow::Result<()> {
    for job in roster() {
        scheduler.add(into_cron_job(job, state.clone())?).await?;
    }

    Ok(())
}

/// Adapts a [`Job`] into a `tokio-cron-scheduler` job.
///
/// This is the single bridge between our trait and the cron engine: the
/// pinned-box closure shape imposed by the crate lives here and nowhere
/// else. The engine's own job type is imported as `CronJob` to keep the
/// two concepts distinct.
fn into_cron_job(job: Arc<dyn Job>, state: AppState) -> Result<CronJob, JobSchedulerError> {
    CronJob::new_async(job.schedule(), move |_id, _scheduler| {
        let job = Arc::clone(&job);
        let state = state.clone();

        Box::pin(async move { execute(job.as_ref(), &state).await })
    })
}

/// Runs a job body and converts its outcome into uniform telemetry.
///
/// Centralizing this guarantees a failing job is logged — never
/// propagated, since an error escaping the closure would compromise the
/// whole cron engine.
async fn execute(job: &dyn Job, state: &AppState) {
    match job.run(state).await {
        Ok(()) => tracing::debug!(job = job.name(), "job completed"),
        Err(error) => tracing::error!(job = job.name(), error = %error, "job failed"),
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests of the private job wiring: the reference `hello` job,
    //! the outcome telemetry of [`execute`] and the schedule validation
    //! in [`into_cron_job`]. The happy registration path is covered end
    //! to end by the engine lifecycle integration test.

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
        assert!(into_cron_job(Arc::new(Failing), state()).is_err());
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
        let job = into_cron_job(Arc::new(Ticking(Arc::clone(&fired))), state())
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
}
