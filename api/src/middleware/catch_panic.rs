//! Last-resort panic recovery.
//!
//! A panicking handler must never tear down the connection with an empty
//! reply: this layer converts the panic into a `500` carrying the standard
//! error envelope, while the full payload goes to the logs. Scope limit:
//! the layer only guards up to the response being produced — a panic
//! while *streaming* a response body would still drop the connection,
//! which is fine while every body is in-memory JSON but must be
//! revisited if a streaming endpoint ever lands.

use std::any::Any;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tower_http::catch_panic::CatchPanicLayer;

/// Named function-pointer type so the layer type stays nameable in the
/// return position of [`layer`].
type PanicHandler = fn(Box<dyn Any + Send + 'static>) -> Response;

/// Layer turning panics into `500` responses with the JSON envelope.
pub fn layer() -> CatchPanicLayer<PanicHandler> {
    CatchPanicLayer::custom(handle_panic as PanicHandler)
}

/// Builds the response for a caught panic.
fn handle_panic(panic: Box<dyn Any + Send + 'static>) -> Response {
    // A panic payload is `String` or `&str` in practice; anything else is
    // opaque and logged as such.
    let detail = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("opaque panic payload");

    // Full detail in the logs only: like every 5xx, the client gets an
    // opaque message so internals never leak.
    tracing::error!(panic = detail, "handler panicked");

    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(json!({ "error": "internal server error" })),
    )
        .into_response()
}
