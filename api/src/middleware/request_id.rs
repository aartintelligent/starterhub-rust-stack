//! Request correlation: `x-request-id` sanitation, generation and
//! propagation.
//!
//! Built on the battle-tested `tower-http` layers rather than a custom
//! implementation. Three pieces, applied at both ends of the stack:
//! [`sanitize`] drops a client-supplied id that could poison the logs,
//! [`set`] assigns a UUID to every request that (then) lacks one, and
//! [`propagate`] copies the id onto the response so clients and
//! upstream proxies can correlate their logs with ours.

use axum::extract::Request;
use axum::http::header::HeaderName;
use axum::middleware::Next;
use axum::response::Response;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};

/// The correlation header, shared by the three middleware pieces.
static X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Upper bound on an accepted client-supplied id. A UUID is 36
/// characters and tracing tokens rarely exceed twice that; the cap only
/// exists so an 8 KB header cannot be replayed into every log line of
/// the request.
const MAX_LENGTH: usize = 128;

/// Drops an incoming `x-request-id` that is oversized or not printable
/// ASCII.
///
/// A legitimate gateway id passes untouched (end-to-end correlation is
/// the point of trusting the header at all), but the header is
/// client-controlled on any directly-exposed deployment: an id gets
/// stamped verbatim into every log line of the request, so junk must be
/// replaced — by removal here, then a fresh UUID from [`set`] — never
/// stored.
pub async fn sanitize(mut request: Request, next: Next) -> Response {
    if let Some(value) = request.headers().get(&X_REQUEST_ID) {
        let acceptable = value.to_str().is_ok_and(|id| {
            !id.is_empty()
                && id.len() <= MAX_LENGTH
                && id.bytes().all(|byte| byte.is_ascii_graphic())
        });

        if !acceptable {
            request.headers_mut().remove(&X_REQUEST_ID);
        }
    }

    next.run(request).await
}

/// Layer assigning a fresh UUID to requests missing an `x-request-id`.
///
/// Incoming ids surviving [`sanitize`] are trusted and kept: when a
/// gateway already tagged the request, overwriting its id would break
/// end-to-end correlation.
pub fn set() -> SetRequestIdLayer<MakeRequestUuid> {
    SetRequestIdLayer::x_request_id(MakeRequestUuid)
}

/// Layer copying the request id onto the response headers.
pub fn propagate() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::x_request_id()
}
