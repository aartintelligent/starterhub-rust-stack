//! Unified error handling for the API.
//!
//! Every handler returns [`ApiResult`], and every failure path converges to
//! [`ApiError`], which knows how to render itself as an HTTP response.
//! Infrastructure errors ([`DbErr`], [`anyhow::Error`]) and extractor
//! rejections ([`JsonRejection`], [`PathRejection`]) convert automatically
//! via `#[from]`, so handlers can simply use the `?` operator and the
//! extractors in [`crate::extract`] reject through the same envelope.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use sea_orm::DbErr;
use serde_json::json;
use thiserror::Error;

/// Convenience alias used as the return type of every handler.
pub type ApiResult<T> = Result<T, ApiError>;

/// All the ways an API call can fail.
///
/// Each variant maps to a single HTTP status code, resolved by the private
/// `status_code` helper below.
#[derive(Debug, Error)]
pub enum ApiError {
    /// The requested resource does not exist (404).
    #[error("resource not found")]
    NotFound,

    /// The request is syntactically valid but semantically wrong (400).
    #[error("{0}")]
    BadRequest(String),

    /// The request conflicts with the current state, e.g. duplicates (409).
    #[error("{0}")]
    Conflict(String),

    /// The JSON body could not be read or deserialized (status carried by
    /// the rejection: 400, 415 or 422). Produced by [`crate::extract::Json`].
    #[error("{}", .0.body_text())]
    JsonRejection(#[from] JsonRejection),

    /// A path parameter could not be parsed (status carried by the
    /// rejection). Produced by [`crate::extract::Path`].
    #[error("{}", .0.body_text())]
    PathRejection(#[from] PathRejection),

    /// A database operation failed (500). Converted from sea-orm errors.
    #[error(transparent)]
    Database(#[from] DbErr),

    /// Any other unexpected failure (500).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    /// Maps the error to its HTTP status code.
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            // Rejections know their own status (400/415/422): forward it
            // instead of flattening every parse failure to one code.
            ApiError::JsonRejection(rejection) => rejection.status(),
            ApiError::PathRejection(rejection) => rejection.status(),
            ApiError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for ApiError {
    /// Renders the error as a JSON body: `{ "error": "<message>" }`.
    ///
    /// Server errors (5xx) are logged with their full cause but exposed to
    /// the client as an opaque message, so internal details never leak.
    fn into_response(self) -> Response {
        let status = self.status_code();

        // Split on the 4xx/5xx boundary: client errors carry an actionable
        // message, server errors are logged in full here — the single choke
        // point — and replaced by an opaque body so SQL, hosts or stack
        // details never reach the client.
        let message = if status.is_server_error() {
            tracing::error!(error = %self, "internal server error");
            "internal server error".to_owned()
        } else {
            self.to_string()
        };

        // Uniform envelope: every failure, whatever its origin, serializes
        // as `{ "error": ... }` so clients parse one single shape.
        (status, Json(json!({ "error": message }))).into_response()
    }
}
