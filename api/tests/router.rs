//! Integration tests of the HTTP surface.
//!
//! Everything is exercised through `tower::ServiceExt::oneshot` against
//! the real router and middleware stack, backed by a mock database: the
//! routing table, the health probes, the JSON error envelope, the
//! documentation gating and the middleware guards (timeout, body cap).

use std::time::Duration;

use api::error::ApiResult;
use api::extract::Json;
use api::middleware;
use api::router::router;
use api::state::AppState;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::routing::{get, post};
use sea_orm::{DatabaseBackend, MockDatabase};
use serde_json::Value;
use tower::util::ServiceExt;

/// Builds the application router backed by a mock database.
fn app(docs: bool) -> Router {
    let conn = MockDatabase::new(DatabaseBackend::Postgres).into_connection();

    router(AppState::new(conn), docs, "under-test")
}

/// Sends `request` through `router` and returns the status plus the body
/// parsed as JSON (`Value::Null` for an empty body, e.g. redirects).
async fn call(router: Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.oneshot(request).await.expect("router is infallible");
    let status = response.status();

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body must be readable");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("every non-empty body must be JSON")
    };

    (status, body)
}

/// Shorthand for an empty-body GET request.
fn get_request(uri: &str) -> Request<Body> {
    Request::get(uri)
        .body(Body::empty())
        .expect("valid request")
}

/// `/livez` answers `200` with the JSON status body.
#[tokio::test]
async fn livez_answers_ok() {
    let (status, body) = call(app(false), get_request("/livez")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

/// `/healthz` is the legacy alias of `/livez` and answers identically.
#[tokio::test]
async fn healthz_aliases_livez() {
    let (status, body) = call(app(false), get_request("/healthz")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

/// `/readyz` pings the (mock) database and answers `200`.
#[tokio::test]
async fn readyz_answers_ok() {
    let (status, body) = call(app(false), get_request("/readyz")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

/// Unknown paths answer `404` through the JSON error envelope, never
/// axum's default empty body.
#[tokio::test]
async fn unknown_path_answers_json_404() {
    let (status, body) = call(app(false), get_request("/definitely-not-a-route")).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "resource not found");
}

/// With documentation enabled, Swagger UI is mounted and the OpenAPI
/// document is served, titled with the runtime identity.
#[tokio::test]
async fn docs_enabled_mounts_swagger_and_contract() {
    let (status, _) = call(app(true), get_request("/docs")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, body) = call(app(true), get_request("/api-docs/openapi.json")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["info"]["title"], "under-test");
    assert!(body["paths"]["/livez"].is_object());
    assert!(body["paths"]["/healthz"].is_object());
    assert!(body["paths"]["/readyz"].is_object());
}

/// With documentation disabled (staging, production), both the UI and
/// the contract disappear behind the JSON `404`.
#[tokio::test]
async fn docs_disabled_hides_swagger_and_contract() {
    for uri in ["/docs", "/api-docs/openapi.json"] {
        let (status, body) = call(app(false), get_request(uri)).await;

        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} must be hidden");
        assert_eq!(body["error"], "resource not found");
    }
}

/// Echoes the JSON body back, exercising the crate-local extractor and
/// its rejection path.
async fn echo(Json(value): Json<Value>) -> ApiResult<Json<Value>> {
    Ok(Json(value))
}

/// Minimal router around [`echo`], wrapped in the full middleware stack
/// with a generous timeout.
fn echo_app() -> Router {
    middleware::apply(
        Router::new().route("/echo", post(echo)),
        Duration::from_secs(5),
    )
}

/// A syntactically invalid JSON body rejects through the `ApiError`
/// envelope with the status carried by the rejection.
#[tokio::test]
async fn malformed_json_rejects_through_envelope() {
    let request = Request::post("/echo")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{ not json"))
        .expect("valid request");

    let (status, body) = call(echo_app(), request).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].is_string(), "rejection must use the envelope");
}

/// A missing JSON content type rejects as `415` through the envelope.
#[tokio::test]
async fn missing_content_type_rejects_through_envelope() {
    let request = Request::post("/echo")
        .body(Body::from("{}"))
        .expect("valid request");

    let (status, body) = call(echo_app(), request).await;

    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(body["error"].is_string(), "rejection must use the envelope");
}

/// A body over the declared cap rejects as `413` through the envelope.
#[tokio::test]
async fn oversized_body_rejects_through_envelope() {
    // 2 MiB of JSON string content plus quotes: just over the cap.
    let oversized = format!("\"{}\"", "x".repeat(2 * 1024 * 1024));
    let request = Request::post("/echo")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(oversized))
        .expect("valid request");

    let (status, body) = call(echo_app(), request).await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert!(body["error"].is_string(), "rejection must use the envelope");
}

/// A handler slower than the configured timeout answers a JSON `408`.
#[tokio::test]
async fn slow_request_answers_json_408() {
    /// Handler sleeping far past the test's timeout.
    async fn slow() -> &'static str {
        tokio::time::sleep(Duration::from_secs(5)).await;
        "too late"
    }

    let app = middleware::apply(
        Router::new().route("/slow", get(slow)),
        Duration::from_millis(50),
    );

    let (status, body) = call(app, get_request("/slow")).await;

    assert_eq!(status, StatusCode::REQUEST_TIMEOUT);
    assert_eq!(body["error"], "request timed out");
}
