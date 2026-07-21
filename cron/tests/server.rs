//! Integration tests of the cron engine lifecycle and the job error type.

use cron::error::JobError;
use cron::server::Server;
use cron::state::AppState;
use sea_orm::{DatabaseBackend, DbErr, MockDatabase};

/// The engine builds with the full roster registered, starts ticking,
/// and stops cleanly once the shutdown future resolves.
#[tokio::test]
async fn engine_starts_and_stops_cleanly() {
    // Global TRACE-level test subscriber (first caller wins), so the
    // lifecycle log statements are fully evaluated.
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();

    let conn = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
    let server = Server::new(AppState::new(conn))
        .await
        .expect("the roster must register");

    // An already-resolved shutdown: the run still walks the whole
    // lifecycle — start, announce, stop — just without waiting.
    server
        .run(std::future::ready(()))
        .await
        .expect("the engine must stop cleanly");
}

/// Infrastructure errors convert into [`JobError`] via `From` and render
/// transparently, so `?` works in every job body.
#[test]
fn infrastructure_errors_convert_into_job_error() {
    let database = JobError::from(DbErr::Custom("boom".into()));
    assert!(database.to_string().contains("boom"));

    let internal = JobError::from(anyhow::anyhow!("boom"));
    assert!(internal.to_string().contains("boom"));
}
