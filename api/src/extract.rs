//! Crate-local extractors whose rejections speak the API error envelope.
//!
//! Axum's stock extractors reject with plain-text bodies, which would leak
//! a second error format to clients. These thin wrappers delegate the
//! actual extraction to axum (`via(...)`) but route every rejection through
//! [`ApiError`], so a malformed payload answers with the same
//! `{ "error": ... }` shape as any other failure.
//!
//! Handlers must always use these instead of `axum::Json` /
//! `axum::extract::Path` for request inputs.

use axum::extract::{FromRequest, FromRequestParts};
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::error::ApiError;

/// JSON body extractor rejecting through [`ApiError`].
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(ApiError))]
pub struct Json<T>(pub T);

/// Path parameters extractor rejecting through [`ApiError`].
#[derive(FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(ApiError))]
pub struct Path<T>(pub T);

impl<T: Serialize> IntoResponse for Json<T> {
    /// Serializes exactly like `axum::Json`, so the same wrapper type is
    /// symmetric: valid for both request extraction and response bodies.
    fn into_response(self) -> Response {
        axum::Json(self.0).into_response()
    }
}
