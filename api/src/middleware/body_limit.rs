//! Request body size cap.
//!
//! axum already enforces a 2 MiB default, but an implicit framework
//! constant is not a reviewable property of the stack: declaring the
//! limit here makes it visible, adjustable and documented. Oversized
//! payloads reject with `413` through the extractor path, so the JSON
//! error envelope holds.

use axum::extract::DefaultBodyLimit;

/// Maximum accepted request body, in bytes — axum's 2 MiB default made
/// explicit.
const MAX_BYTES: usize = 2 * 1024 * 1024;

/// Layer declaring the request body size cap consumed by the extractors.
pub fn layer() -> DefaultBodyLimit {
    DefaultBodyLimit::max(MAX_BYTES)
}
