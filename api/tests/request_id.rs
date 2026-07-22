//! Integration tests of the request-correlation middleware: id
//! generation, trusted pass-through, sanitation of hostile values, and
//! propagation onto middleware-synthesized responses.

use std::time::Duration;

use api::middleware;
use axum::Router;
use axum::body::Body;
use axum::http::{HeaderValue, Request, StatusCode};
use axum::routing::get;
use tower::util::ServiceExt;

/// Minimal router wrapped in the full middleware stack.
fn app() -> Router {
    middleware::apply(
        Router::new().route("/ping", get(|| async { "pong" })),
        Duration::from_secs(5),
    )
}

/// Sends a request and returns the response's `x-request-id` value plus
/// the status.
async fn round_trip(request: Request<Body>) -> (StatusCode, Option<String>) {
    let response = app().oneshot(request).await.expect("router is infallible");
    let status = response.status();
    let id = response
        .headers()
        .get("x-request-id")
        .map(|value| value.to_str().expect("the id must be ASCII").to_owned());

    (status, id)
}

/// A request without an id gets a fresh UUID, mirrored on the response.
#[tokio::test]
async fn missing_id_gets_a_uuid() {
    let request = Request::get("/ping")
        .body(Body::empty())
        .expect("valid request");

    let (status, id) = round_trip(request).await;

    assert_eq!(status, StatusCode::OK);
    let id = id.expect("the response must carry an id");
    assert_eq!(id.len(), 36, "a UUID is 36 characters: {id}");
}

/// A sane client-supplied id is preserved end to end: overwriting a
/// gateway's id would break upstream correlation.
#[tokio::test]
async fn sane_client_id_is_preserved() {
    let request = Request::get("/ping")
        .header("x-request-id", "gateway-abc-123")
        .body(Body::empty())
        .expect("valid request");

    let (_, id) = round_trip(request).await;

    assert_eq!(id.as_deref(), Some("gateway-abc-123"));
}

/// An oversized id is replaced by a fresh UUID instead of being stamped
/// into every log line of the request.
#[tokio::test]
async fn oversized_id_is_replaced() {
    let oversized = "x".repeat(500);
    let request = Request::get("/ping")
        .header("x-request-id", &oversized)
        .body(Body::empty())
        .expect("valid request");

    let (_, id) = round_trip(request).await;

    let id = id.expect("the response must carry an id");
    assert_ne!(id, oversized);
    assert_eq!(id.len(), 36, "a fresh UUID must replace the junk: {id}");
}

/// An id with non-printable bytes is replaced as well.
#[tokio::test]
async fn non_printable_id_is_replaced() {
    let hostile = HeaderValue::from_bytes(b"tab\there").expect("a tab is a legal header byte");
    let request = Request::get("/ping")
        .header("x-request-id", hostile)
        .body(Body::empty())
        .expect("valid request");

    let (_, id) = round_trip(request).await;

    let id = id.expect("the response must carry an id");
    assert_eq!(id.len(), 36, "a fresh UUID must replace the junk: {id}");
}

/// The id survives onto middleware-synthesized responses — here the
/// timeout `503` — because `propagate` sits above the timeout handler.
/// Regression guard for the stack ordering in `middleware::apply`.
#[tokio::test]
async fn id_survives_onto_timeout_responses() {
    /// Handler sleeping far past the test's timeout.
    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_secs(5)).await;
        "too late"
    }

    let app = middleware::apply(
        Router::new().route("/slow", get(slow)),
        Duration::from_millis(50),
    );
    let request = Request::get("/slow")
        .header("x-request-id", "timeout-correlation-1")
        .body(Body::empty())
        .expect("valid request");

    let response = app.oneshot(request).await.expect("router is infallible");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok()),
        Some("timeout-correlation-1")
    );
}
