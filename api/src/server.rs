//! HTTP server bootstrap.
//!
//! Decouples "how the API starts" from "what the application boots":
//! the binary crate decides the address, provides the database connection
//! and the shutdown future; this module does the rest — router assembly,
//! socket tuning and the graceful serve loop.
//!
//! The serve loop is hand-rolled on `hyper-util` rather than
//! `axum::serve`, for one reason: axum's loop exposes no header-read
//! timeout, so a client sending its request head one byte at a time
//! (slowloris) would hold a task forever — the per-request
//! `TimeoutLayer` only starts once the head is fully parsed.

use std::future::Future;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder as ConnectionBuilder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use sea_orm::DatabaseConnection;
use tokio::net::TcpListener;

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
        let listener = TcpListener::bind(&self.addr)
            .await
            .with_context(|| format!("failed to bind {}", self.addr))?;

        // Resolve the bound address eagerly so it is available before the
        // accept loop starts — the serve step only has to announce it.
        let local_addr = listener
            .local_addr()
            .context("failed to read the bound local address")?;

        Ok(BoundServer {
            app,
            listener,
            local_addr,
            timeout: self.timeout,
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
    /// Listener owning the bound socket.
    listener: TcpListener,
    /// Address actually bound, resolved at bind time.
    local_addr: SocketAddr,
    /// Per-request budget, reused as the header-read budget: a client
    /// unable to finish sending its request head within the time a whole
    /// request may take is not a client worth waiting for.
    timeout: Duration,
}

impl BoundServer {
    /// Address actually bound — the ground truth when the configured
    /// address used port 0 to let the OS pick a free one.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serves requests until `shutdown` resolves.
    ///
    /// Shutdown is graceful: the loop stops accepting new connections,
    /// tells every open connection to finish its in-flight request and
    /// close (keep-alive included), then returns once all have drained.
    ///
    /// # Errors
    ///
    /// Reserved for future serve-loop failures; accept errors are
    /// transient (the kernel keeps the listener alive) and only logged.
    pub async fn serve(
        self,
        shutdown: impl Future<Output = ()> + Send + 'static,
    ) -> anyhow::Result<()> {
        // Log the address actually bound, not the configured one: this
        // line is what operators and tests rely on.
        tracing::info!("listening on {}", self.local_addr);

        // One protocol builder for every connection. The header-read
        // timeout is the slowloris guard this whole hand-rolled loop
        // exists for; both protocol stacks get a timer, without which
        // hyper's timeout configuration panics at runtime.
        let mut builder = ConnectionBuilder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(self.timeout);
        builder.http2().timer(TokioTimer::new());

        // The graceful registry: every accepted connection is watched,
        // and `shutdown()` below resolves once the last one drained.
        let graceful = GracefulShutdown::new();
        let service = TowerToHyperService::new(self.app);

        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (stream, _peer) = match accepted {
                        Ok(connection) => connection,
                        // Transient by nature (EMFILE, aborted handshake):
                        // the kernel keeps the listener alive, so log and
                        // keep accepting instead of killing the server.
                        Err(error) => {
                            tracing::debug!(%error, "accept failed");
                            continue;
                        }
                    };

                    // Nagle's algorithm batches small writes at the cost
                    // of latency; an API answering small JSON bodies is
                    // exactly the workload it hurts. A failure only loses
                    // an optimization, never the connection.
                    if let Err(error) = stream.set_nodelay(true) {
                        tracing::debug!(%error, "failed to set TCP_NODELAY");
                    }

                    // One task per connection, watched by the graceful
                    // registry; `into_owned` detaches the connection from
                    // the builder's lifetime so the task is 'static.
                    let connection = graceful.watch(
                        builder
                            .serve_connection_with_upgrades(TokioIo::new(stream), service.clone())
                            .into_owned(),
                    );
                    tokio::spawn(async move {
                        // Connection-level errors (client reset, malformed
                        // HTTP, the header-read timeout firing) are the
                        // client's problem, not the server's: debug, not
                        // error, or a port scan would flood the logs.
                        if let Err(error) = connection.await {
                            tracing::debug!(error = %error, "connection ended with an error");
                        }
                    });
                }
                () = &mut shutdown => break,
            }
        }

        // Close the listener first so no new connection sneaks in while
        // the open ones drain.
        drop(self.listener);
        tracing::info!("http server draining connections");
        graceful.shutdown().await;
        tracing::info!("http server stopped");

        Ok(())
    }
}
