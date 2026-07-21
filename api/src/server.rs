//! HTTP server bootstrap.
//!
//! Decouples "how the API starts" from "what the application boots":
//! the binary crate decides the address, provides the database connection
//! and the shutdown future; this module does the rest — router assembly,
//! socket tuning and the graceful serve loop.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use axum::serve::Listener;
use sea_orm::DatabaseConnection;
use tokio::net::{TcpListener, TcpStream};

use crate::middleware;
use crate::router;
use crate::state::AppState;

/// A configured, ready-to-run HTTP server.
pub struct Server {
    /// Address to bind, e.g. `127.0.0.1:8080`.
    addr: String,
    /// Database handle shared with every handler through [`AppState`].
    conn: DatabaseConnection,
    /// Whether the interactive documentation (`/docs`) is mounted; the
    /// binary decides from the deployment environment.
    docs: bool,
    /// Upper bound on the total processing time of one request.
    timeout: Duration,
}

impl Server {
    /// Creates a server bound to `addr` once [`run`](Self::run) is called.
    ///
    /// The server stays configuration-agnostic on purpose: it receives
    /// plain values (`docs`, `timeout`), and the binary crate maps the
    /// configuration onto them.
    pub fn new(
        addr: impl Into<String>,
        conn: DatabaseConnection,
        docs: bool,
        timeout: Duration,
    ) -> Self {
        Self {
            addr: addr.into(),
            conn,
            docs,
            timeout,
        }
    }

    /// Binds the listener and serves requests until `shutdown` resolves.
    ///
    /// Shutdown is graceful: axum stops accepting new connections, lets
    /// in-flight requests complete, then returns.
    ///
    /// # Errors
    ///
    /// Fails if the address cannot be bound or if the server loop aborts;
    /// every error carries enough context to be actionable from the logs.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        // Assemble the full router here, not in the constructor: `run`
        // consumes `self`, so the state is built exactly once and no
        // half-initialized server can ever be observed. The middleware
        // stack wraps the finished router so it covers every route.
        let app = middleware::apply(
            router::router(AppState::new(self.conn), self.docs),
            self.timeout,
        );

        // Bind before announcing anything: if the port is taken or the
        // interface is invalid, we fail fast with an error naming the
        // culprit address instead of logging a misleading "listening" line.
        let listener = TcpListener::bind(&self.addr)
            .await
            .with_context(|| format!("failed to bind {}", self.addr))?;

        // Log the address actually bound, not the configured one: with
        // port 0 the OS picks a free port and this line is the ground
        // truth operators and tests rely on.
        let local_addr = listener
            .local_addr()
            .context("failed to read the bound local address")?;
        tracing::info!("listening on {local_addr}");

        // Enter the accept loop; the future resolves once `shutdown` fires
        // and every in-flight request has been given a chance to finish.
        axum::serve(NoDelayListener(listener), app)
            .with_graceful_shutdown(shutdown)
            .await
            .context("http server aborted")?;

        tracing::info!("http server stopped");

        Ok(())
    }
}

/// TCP listener enabling `TCP_NODELAY` on every accepted connection.
///
/// Nagle's algorithm batches small writes at the cost of latency; an API
/// answering small JSON bodies is exactly the workload it hurts. Disabling
/// it per accepted socket is the standard production tuning, and wrapping
/// the listener is the one place that catches every connection.
struct NoDelayListener(TcpListener);

impl Listener for NoDelayListener {
    type Io = TcpStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        // Delegate to Tokio's listener (via axum's impl, which already
        // retries transient accept errors), then tune the socket.
        let (stream, addr) = Listener::accept(&mut self.0).await;

        // A failure here only loses an optimization, never the connection:
        // log at debug and keep serving.
        if let Err(error) = stream.set_nodelay(true) {
            tracing::debug!(%error, "failed to set TCP_NODELAY");
        }

        (stream, addr)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.0.local_addr()
    }
}
