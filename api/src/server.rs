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
use axum::Router;
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
    /// Runtime application identity, used as the OpenAPI document title.
    name: String,
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
    /// plain values (`name`, `docs`, `timeout`), and the binary crate
    /// maps the configuration onto them.
    pub fn new(
        addr: impl Into<String>,
        conn: DatabaseConnection,
        name: impl Into<String>,
        docs: bool,
        timeout: Duration,
    ) -> Self {
        Self {
            addr: addr.into(),
            conn,
            name: name.into(),
            docs,
            timeout,
        }
    }

    /// Assembles the router and binds the listener, without serving yet.
    ///
    /// Splitting bind from serve keeps startup failures (port taken,
    /// invalid interface) distinct from the accept loop and exposes the
    /// address actually bound: with port 0 the OS picks a free port and
    /// [`BoundServer::local_addr`] is the ground truth operators and
    /// tests rely on.
    ///
    /// # Errors
    ///
    /// Fails if the address cannot be bound; the error names the culprit
    /// address so the failure is actionable from the logs.
    pub async fn bind(self) -> anyhow::Result<BoundServer> {
        // Assemble the full router here, not in the constructor: `bind`
        // consumes `self`, so the state is built exactly once and no
        // half-initialized server can ever be observed. The middleware
        // stack wraps the finished router so it covers every route.
        let app = middleware::apply(
            router::router(AppState::new(self.conn), self.docs, &self.name),
            self.timeout,
        );

        // Bind before announcing anything: if the port is taken or the
        // interface is invalid, we fail fast with an error naming the
        // culprit address instead of logging a misleading "listening" line.
        let listener = NoDelayListener(
            TcpListener::bind(&self.addr)
                .await
                .with_context(|| format!("failed to bind {}", self.addr))?,
        );

        // Resolve the bound address eagerly so it is available before the
        // accept loop starts — the serve step only has to announce it.
        let local_addr = listener
            .local_addr()
            .context("failed to read the bound local address")?;

        Ok(BoundServer {
            app,
            listener,
            local_addr,
        })
    }

    /// Binds the listener and serves requests until `shutdown` resolves.
    ///
    /// Convenience over [`bind`](Self::bind) then
    /// [`serve`](BoundServer::serve) for callers that do not need the
    /// bound address.
    ///
    /// # Errors
    ///
    /// Fails if the address cannot be bound or if the server loop aborts;
    /// every error carries enough context to be actionable from the logs.
    pub async fn run(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        self.bind().await?.serve(shutdown).await
    }
}

/// A server whose listener is bound, ready to enter the accept loop.
pub struct BoundServer {
    /// Full application router wrapped in the middleware stack.
    app: Router,
    /// Tuned listener owning the bound socket.
    listener: NoDelayListener,
    /// Address actually bound, resolved at bind time.
    local_addr: SocketAddr,
}

impl BoundServer {
    /// Address actually bound — the ground truth when the configured
    /// address used port 0 to let the OS pick a free one.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serves requests until `shutdown` resolves.
    ///
    /// Shutdown is graceful: axum stops accepting new connections, lets
    /// in-flight requests complete, then returns.
    ///
    /// # Errors
    ///
    /// Fails if the server loop aborts; the error carries enough context
    /// to be actionable from the logs.
    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        // Log the address actually bound, not the configured one: this
        // line is what operators and tests rely on.
        tracing::info!("listening on {}", self.local_addr);

        // Enter the accept loop; the future resolves once `shutdown` fires
        // and every in-flight request has been given a chance to finish.
        axum::serve(self.listener, self.app)
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
