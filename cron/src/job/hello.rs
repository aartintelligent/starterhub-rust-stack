//! Hello job: the simplest possible job, a log line per tick.
//!
//! Serves as the reference implementation for new jobs: copy this file,
//! adjust the name, schedule and `run` body, then add the struct to
//! [`crate::job::roster`].

use crate::error::JobResult;
use crate::job::Job;
use crate::state::AppState;

/// Unit struct carrying the job implementation.
pub(crate) struct Hello;

#[async_trait::async_trait]
impl Job for Hello {
    fn name(&self) -> &'static str {
        "hello"
    }

    /// Every minute, at second 0.
    fn schedule(&self) -> &'static str {
        "0 * * * * * *"
    }

    async fn run(&self, _state: &AppState) -> JobResult {
        // A single log line is the whole job: enough to see the cron
        // engine ticking. INFO on purpose: this is the template's sample
        // job, meant to be visible out of the box — anyone building on
        // the template replaces it (and its noise) with real jobs.
        tracing::info!("hello from the cron engine");

        Ok(())
    }
}
