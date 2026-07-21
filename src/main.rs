//! Application entry point.
//!
//! Wires the crates of the workspace together and drives the boot
//! sequence: telemetry, configuration, database, migrations, then the two
//! long-lived components — HTTP API and cron engine — spawned side by
//! side under a shared [`CancellationToken`]. The token is the single
//! source of truth for shutdown: it fires on SIGTERM/SIGINT (the signals a
//! Debian service receives from systemd) **and** when any component stops
//! on its own, so the process can never linger half-alive.

use std::time::Duration;

use anyhow::Context;
use api::server::Server as ApiServer;
use common::{config::Config, infrastructure::postgresql, telemetry};
use cron::server::Server as CronServer;
use cron::state::AppState;
use migration::{Migrator, MigratorTrait};
use tokio::signal::ctrl_c;
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Boots the application.
///
/// Steps, in order:
/// 1. Load the layered configuration (defaults, then the optional
///    `app-config.json` files, then `APP_*` env).
/// 2. Install the global tracing subscriber, verbosity driven by `APP_DEBUG`.
/// 3. Open the PostgreSQL connection pool.
/// 4. Apply pending migrations so the schema is always up to date.
/// 5. Spawn the HTTP server and the cron engine under one cancellation
///    token, then supervise both until the process stops.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Resolve the configuration first: every subsequent step depends on it,
    // and a malformed source must abort the boot before anything starts.
    // The env! macros expand HERE, in the binary crate, so the identity
    // defaults are the real executable name and version from Cargo.toml.
    let config = Config::load(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))?;

    // Install telemetry as early as possible so every later step (pool
    // creation, migrations, server, cron engine) is traced from its very
    // first event.
    telemetry::init(config.debug);

    // First trace of every boot: which build is running, and where.
    // Invaluable in production logs when correlating an incident with a
    // deployment.
    tracing::info!(
        name = %config.name,
        version = %config.version,
        environment = ?config.environment,
        "booting"
    );

    // Open the PostgreSQL pool eagerly: failing fast at boot beats
    // discovering a broken database on the first incoming request. `?`
    // with context, not `expect`: a boot failure must exit non-zero with
    // a clean, greppable error line, never a panic backtrace.
    let conn = postgresql::connect(&config.database)
        .await
        .context("failed to connect to PostgreSQL")?;

    // Bring the schema up to date before accepting any traffic, so the
    // running code and the database are always aligned.
    Migrator::up(&conn, None)
        .await
        .context("failed to apply database migrations")?;

    // Build the cron engine before spawning anything: an invalid
    // hard-coded schedule must abort the boot, not fail silently in a
    // job. Jobs get their own state over a clone of the pool handle.
    let cron = CronServer::new(AppState::new(conn.clone())).await?;

    // Single source of truth for shutdown: every component watches this
    // token instead of installing its own signal handlers, so "stop" has
    // exactly one definition process-wide.
    let shutdown = CancellationToken::new();

    // Bridge Unix signals to the token from a dedicated task; it needs no
    // supervision, cancelling is idempotent and the task dies with the
    // process.
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown_signal().await;
            shutdown.cancel();
        }
    });

    // Spawn each long-lived component with its own view of the token;
    // `canceled_owned` yields a plain `Future<Output = ()>`, so the
    // components stay signal-agnostic and testable with any future.
    // The API receives plain values, not the configuration itself: the
    // environment collapses into "expose the docs or not", the timeout
    // into a Duration, and the identity into the OpenAPI title.
    let api_handle = tokio::spawn(
        ApiServer::new(
            config.server.url(),
            conn,
            config.name.clone(),
            config.environment.exposes_docs(),
            Duration::from_secs(config.server.timeout),
        )
        .run(shutdown.clone().cancelled_owned()),
    );
    let cron_handle = tokio::spawn(cron.run(shutdown.clone().cancelled_owned()));

    // Supervise both: whichever stops first — crash, error or graceful —
    // cancels the token so the sibling stops too. `join!` (not `try_join!`)
    // then guarantees both components fully terminated before we return,
    // which is what makes the shutdown actually graceful.
    let (api_result, cron_result) = tokio::join!(
        supervise("http-server", api_handle, &shutdown),
        supervise("cron", cron_handle, &shutdown),
    );

    // Surface the first failure only after both components are down: exit
    // code and logs then reflect the root cause, not a shutdown artefact.
    api_result.and(cron_result)?;

    // Both components stopped cleanly: log it so operators can tell a
    // graceful stop from a crash in the journal.
    tracing::info!("shutdown complete");

    Ok(())
}

/// Awaits one component and guarantees its siblings stop with it.
///
/// Converts the two failure layers into one labeled result: a `JoinError`
/// (the component panicked or was aborted) and the component's own error.
/// In every case the shared token is canceled — a process with only half
/// its components alive must never keep running.
async fn supervise(
    name: &'static str,
    handle: JoinHandle<anyhow::Result<()>>,
    shutdown: &CancellationToken,
) -> anyhow::Result<()> {
    let result = match handle.await {
        Ok(result) => result.with_context(|| format!("{name} failed")),
        Err(join_error) => {
            Err(anyhow::Error::from(join_error)).with_context(|| format!("{name} panicked"))
        }
    };

    // Idempotent: on a signal-initiated stop the token is already
    // canceled and this is a no-op; on a crash it is what stops the
    // sibling component.
    shutdown.cancel();

    result
}

/// Resolves once the process receives SIGTERM or SIGINT.
///
/// SIGTERM is what systemd sends on `systemctl stop` (Debian deployment
/// target); SIGINT covers Ctrl-C during local development. Installing a
/// handler only fails on exotic setups (e.g. no signal driver): better to
/// crash than to run unstoppable.
async fn shutdown_signal() {
    // Two symmetric one-shot futures: `ctrl_c` is Tokio's portable SIGINT
    // helper, SIGTERM goes through the Unix API. Each block installs its
    // handler and waits, so nothing outlives the select below.
    let sigint = async {
        ctrl_c().await.expect("failed to install SIGINT handler");
    };
    let sigterm = async {
        signal(SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    // First signal wins; which one arrived is worth a log line because it
    // tells operators who initiated the stop.
    tokio::select! {
        () = sigint => tracing::info!("received SIGINT, shutting down"),
        () = sigterm => tracing::info!("received SIGTERM, shutting down"),
    }
}
