//! Tests of the [`ApiError`] envelope: one variant per status code, and
//! the 4xx/5xx masking boundary — client errors carry their message,
//! server errors are always replaced by an opaque body.

use api::error::ApiError;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use sea_orm::DbErr;
use serde_json::Value;

/// Installs a global TRACE-level test subscriber (first caller wins) so
/// the log statements of the exercised paths are fully evaluated.
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_test_writer()
        .try_init();
}

/// Renders `error` and returns the status plus the JSON body.
async fn render(error: ApiError) -> (StatusCode, Value) {
    init_tracing();

    let response = error.into_response();
    let status = response.status();

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body must be readable");
    let body = serde_json::from_slice(&bytes).expect("every error body must be JSON");

    (status, body)
}

/// `NotFound` renders `404` with its fixed message.
#[tokio::test]
async fn not_found_renders_404() {
    let (status, body) = render(ApiError::NotFound).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "resource not found");
}

/// `BadRequest` renders `400` and carries its actionable message.
#[tokio::test]
async fn bad_request_carries_its_message() {
    let (status, body) = render(ApiError::BadRequest("subnet mask is invalid".into())).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "subnet mask is invalid");
}

/// `Conflict` renders `409` and carries its actionable message.
#[tokio::test]
async fn conflict_carries_its_message() {
    let (status, body) = render(ApiError::Conflict("name already taken".into())).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], "name already taken");
}

/// A database error converts via `From`, renders `500`, and its detail
/// never reaches the client.
#[tokio::test]
async fn database_error_is_masked() {
    let (status, body) = render(ApiError::from(DbErr::Custom("secret detail".into()))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "internal server error");
}

/// Any other unexpected failure converts via `From`, renders `500`, and
/// its detail never reaches the client.
#[tokio::test]
async fn internal_error_is_masked() {
    let (status, body) = render(ApiError::from(anyhow::anyhow!("secret detail"))).await;

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body["error"], "internal server error");
}
