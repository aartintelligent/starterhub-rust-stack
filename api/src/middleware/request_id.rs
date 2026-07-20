//! Request correlation: `x-request-id` generation and propagation.
//!
//! Built on the battle-tested `tower-http` layers rather than a custom
//! implementation. Two halves, applied at both ends of the stack:
//! [`set`] assigns a UUID to every incoming request that does not already
//! carry one, [`propagate`] copies it onto the response so clients and
//! upstream proxies can correlate their logs with ours.

use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

/// Layer assigning a fresh UUID to requests missing an `x-request-id`.
///
/// Incoming ids are trusted and kept: when a gateway already tagged the
/// request, overwriting its id would break end-to-end correlation.
pub fn set() -> SetRequestIdLayer<MakeRequestUuid> {
    SetRequestIdLayer::x_request_id(MakeRequestUuid)
}

/// Layer copying the request id onto the response headers.
pub fn propagate() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::x_request_id()
}
