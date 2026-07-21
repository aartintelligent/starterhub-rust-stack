//! Cross-cutting axum middlewares.
//!
//! Conventions, following the axum/tower guidance:
//!
//! - One module per concern: a layer does one thing well; concerns are
//!   composed, never merged into a single layer.
//! - Reach for `tower-http`'s battle-tested layers before writing custom
//!   ones; use `axum::middleware::from_fn` for simple internal logic, and
//!   a full `tower::Layer`/`Service` pair only when the middleware needs
//!   builder-style configuration or is meant to be published.
//! - The whole stack is assembled once, in [`apply`], through
//!   `tower::ServiceBuilder`: layers execute **top to bottom**, which reads
//!   naturally (unlike chained `Router::layer` calls, which execute bottom
//!   to top).

mod catch_panic;
mod request_id;
mod timeout;
mod trace;

use std::time::Duration;

use axum::Router;
use tower::ServiceBuilder;

/// Applies the full middleware stack to the router.
///
/// Order matters and is intentional:
/// 1. [`request_id::set`] first, so every later layer sees the id;
/// 2. [`trace::layer`] next, opening a span already carrying that id;
/// 3. [`request_id::propagate`] then mirrors the id onto the response;
/// 4. [`timeout::handle`] + [`timeout::layer`] bound the time spent in
///    everything below, answering a JSON `408` past `timeout`;
/// 5. [`catch_panic::layer`] innermost, so a panicking handler still
///    produces a traced, correlated `500` instead of a dropped connection.
pub fn apply(router: Router, timeout: Duration) -> Router {
    router.layer(
        ServiceBuilder::new()
            .layer(request_id::set())
            .layer(trace::layer())
            .layer(request_id::propagate())
            .layer(timeout::handle())
            .layer(timeout::layer(timeout))
            .layer(catch_panic::layer()),
    )
}
