//! Per-request execution time limit.
//!
//! Bounds the total time a request may spend in the stack: a hung
//! dependency must become a fast, explicit failure instead of a
//! connection held open indefinitely. Built on `tower`'s timeout plus
//! axum's error-handling bridge — `tower-http`'s own timeout layer
//! answers with an empty body, which would break the JSON-only rule.

use std::future::{Ready, ready};
use std::time::Duration;

use axum::BoxError;
use axum::error_handling::HandleErrorLayer;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tower::timeout::TimeoutLayer;
use tower::timeout::error::Elapsed;

/// Named function-pointer type so the handler layer stays nameable in the
/// return position of [`handle`].
type TimeoutErrorHandler = fn(BoxError) -> Ready<Response>;

/// Layer enforcing the time limit.
///
/// Its service becomes fallible, which axum rejects: it must always be
/// wrapped by [`handle`] directly above it in the stack.
pub fn layer(timeout: Duration) -> TimeoutLayer {
    TimeoutLayer::new(timeout)
}

/// Layer converting the error raised by [`layer`] back into an
/// infallible response carrying the standard JSON envelope.
pub fn handle() -> HandleErrorLayer<TimeoutErrorHandler, ()> {
    HandleErrorLayer::new(on_error as TimeoutErrorHandler)
}

/// Maps an error surfacing under [`handle`] to a response.
fn on_error(error: BoxError) -> Ready<Response> {
    // The timeout is the only fallible layer underneath: anything else is
    // unexpected and masked as an opaque 500, like every other 5xx.
    let response = if error.is::<Elapsed>() {
        // `warn`, not `error`: the interesting failure (the slow
        // dependency) is already traced by the request span; this line
        // only records that the guard fired.
        tracing::warn!("request timed out");

        (
            StatusCode::REQUEST_TIMEOUT,
            axum::Json(json!({ "error": "request timed out" })),
        )
    } else {
        tracing::error!(error = %error, "internal server error");

        (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({ "error": "internal server error" })),
        )
    };

    ready(response.into_response())
}

#[cfg(test)]
mod tests {
    //! The timeout branch (408) is covered end to end by the router
    //! integration tests; only the defensive non-timeout branch needs a
    //! direct call — nothing in the real stack can produce it.

    use super::*;

    /// Any error other than `Elapsed` is masked as an opaque `500`.
    #[tokio::test]
    async fn unexpected_error_masks_as_500() {
        // Global TRACE-level test subscriber (first caller wins), so the
        // masking log statement is fully evaluated.
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();

        let response = on_error(Box::new(std::io::Error::other("secret detail"))).await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
