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
