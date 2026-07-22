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
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// Grace between the shutdown order and a forced exit. Longer than every
/// component-level drain fence (the cron engine waits at most 30 s for
/// in-flight jobs, the API bounds its connections by the request
/// timeout), so this fence only fires on a genuine hang — and the
/// process still dies deterministically instead of waiting for
/// systemd's SIGKILL.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(45);

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
    // Populate the process environment from `./.env` first: loading
    // environment files is the executable's concern, kept out of
    // `Config::load` so the library stays pure and its tests hermetic.
    // A malformed `.env` aborts the boot loudly.
    common::config::load_dotenv()?;

    // Resolve the configuration next: every subsequent step depends on it,
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
    // running code and the database are always aligned — under an
    // advisory lock, so concurrently booting replicas serialize instead
    // of colliding on the same pending migration. Signal handling is not
    // installed yet, so a SIGTERM here kills the process via the default
    // disposition — safe, because the whole run is transactional: the
    // schema is migrated or not, never half-way.
    migration::up_guarded(&conn)
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

    // Install both signal streams HERE, before anything is spawned: if
    // the signal driver is broken, the boot must fail with a clean error
    // — not panic later inside a detached task, which would leave the
    // process running with graceful stop permanently impossible (tokio
    // signal registration is process-wide and irreversible).
    let sigint = signal(SignalKind::interrupt()).context("failed to install the SIGINT handler")?;
    let sigterm =
        signal(SignalKind::terminate()).context("failed to install the SIGTERM handler")?;

    // Bridge Unix signals to the token from a dedicated task; it needs no
    // supervision, cancelling is idempotent and the task dies with the
    // process.
    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            let (mut sigint, mut sigterm) = (sigint, sigterm);

            // First signal: order the graceful stop. Which one arrived
            // is worth a log line because it tells operators who
            // initiated it.
            tokio::select! {
                _ = sigint.recv() => tracing::info!("received SIGINT, shutting down"),
                _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down"),
            }
            shutdown.cancel();

            // Second signal: the operator insists — exit right now.
            // Without this escalation, every further Ctrl-C would be
            // silently swallowed (the handlers above are permanent) and
            // a hung shutdown could only be ended by SIGKILL from
            // another terminal.
            tokio::select! {
                _ = sigint.recv() => {}
                _ = sigterm.recv() => {}
            }
            tracing::error!("second signal received, exiting immediately");
            std::process::exit(130);
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
    // which is what makes the shutdown actually graceful. The whole wait
    // is fenced: once the shutdown order is out, components get
    // `SHUTDOWN_GRACE` to drain before the process exits anyway — a hung
    // component must never turn "systemctl stop" into a SIGKILL timeout.
    let (api_result, cron_result) = tokio::select! {
        results = async {
            tokio::join!(
                supervise("http-server", api_handle, &shutdown),
                supervise("cron", cron_handle, &shutdown),
            )
        } => results,
        () = async {
            shutdown.cancelled().await;
            tokio::time::sleep(SHUTDOWN_GRACE).await;
        } => {
            tracing::error!(grace = ?SHUTDOWN_GRACE, "components still running after the shutdown grace, exiting");
            std::process::exit(1);
        }
    };

    // Surface a failure only after both components are down. Both errors
    // were already logged individually by `supervise`, so nothing is
    // lost when only one of them can become the exit status.
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

    // Log the failure here, per component: when both fail, only one
    // error can become the process exit status — this line is what
    // preserves the other one (potentially the root cause).
    if let Err(error) = &result {
        tracing::error!(component = name, error = %format_args!("{error:#}"), "component stopped with an error");
    }

    // Idempotent: on a signal-initiated stop the token is already
    // canceled and this is a no-op; on a crash it is what stops the
    // sibling component.
    shutdown.cancel();

    result
}
