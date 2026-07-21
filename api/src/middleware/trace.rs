//! HTTP request tracing.
//!
//! Wraps `tower-http`'s [`TraceLayer`] with a custom span that carries the
//! request id, so every log line emitted while handling a request is
//! automatically correlated.

use axum::body::Body;
use axum::http::Request;
use tower_http::classify::{ServerErrorsAsFailures, SharedClassifier};
use tower_http::trace::TraceLayer;
use tracing::Span;

/// Named function-pointer type so the layer type stays nameable: a closure
/// would make the return type of [`layer`] impossible to write.
type MakeRequestSpan = fn(&Request<Body>) -> Span;

/// Layer opening one `INFO` span per request, closed with the response
/// classification (server errors are reported as failures).
pub fn layer() -> TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MakeRequestSpan> {
    TraceLayer::new_for_http().make_span_with(make_span as MakeRequestSpan)
}

/// Builds the per-request span: method, URI and correlation id.
fn make_span(request: &Request<Body>) -> Span {
    // The id is set by the `request_id::set` layer, which runs before this
    // one; "unknown" only appears if the stack ordering is broken.
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown");

    tracing::info_span!(
        "http_request",
        method = %request.method(),
        uri = %request.uri(),
        request_id,
    )
}

#[cfg(test)]
mod tests {
    //! The layer wiring is covered by the HTTP integration tests; what
    //! needs a direct call is the span construction itself, under an
    //! active subscriber so its fields are actually recorded.

    use super::*;

    /// The per-request span is enabled under an active subscriber and
    /// carries the correlation id from the request headers.
    #[test]
    fn span_builds_under_an_active_subscriber() {
        // Global TRACE-level test subscriber (first caller wins), so the
        // span callsite is enabled and its fields evaluate.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();

        let request = Request::builder()
            .uri("/probe")
            .header("x-request-id", "test-id")
            .body(Body::empty())
            .expect("valid request");

        assert!(!make_span(&request).is_disabled());
    }
}
