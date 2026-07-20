//! Router assembly.
//!
//! The single place where URLs are mapped to handlers. Group new endpoints
//! by resource and prefer `Router::nest` as the surface grows.
//!
//! Technical endpoints — Kubernetes health probes and the 404 fallback —
//! are defined here as well: they are properties of the routing table
//! itself, while business handlers live in [`crate::handler`].

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{Router, routing::get};
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::state::AppState;

/// Builds the top-level router with `state` attached.
///
/// Health endpoints follow the Kubernetes probe conventions: `/livez` and
/// its legacy alias `/healthz` for liveness, `/readyz` for readiness.
pub fn router(state: AppState) -> Router {
    Router::new()
        // Probes stay at the root, outside any versioned prefix: their
        // paths are contractual for the orchestrator and must survive API
        // evolutions. `/healthz` is kept as the legacy alias of `/livez`.
        .route("/healthz", get(livez))
        .route("/livez", get(livez))
        .route("/readyz", get(readyz))
        // Unknown paths answer with the JSON error envelope instead of
        // axum's default empty 404.
        .fallback(not_found)
        // Attach the state last so every route above receives it; axum
        // enforces at compile time that nothing is left unresolved.
        .with_state(state)
}

/// Liveness probe: reports that the process is up and able to answer.
///
/// Never checks dependencies — a failing dependency must not get the pod
/// restarted; that distinction belongs to [`readyz`]. Kubernetes only
/// interprets the status code; the body is JSON like every other response
/// of this API — no client should ever need a second content type.
async fn livez() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

/// Readiness probe: reports whether the service can handle traffic.
///
/// Pings the database through the shared pool; a failure yields
/// `503 Service Unavailable` so the pod is removed from load balancing
/// without being restarted.
async fn readyz(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    // A pool-level ping is the cheapest end-to-end proof that a connection
    // can be acquired and the server answers.
    match state.conn.ping().await {
        Ok(()) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(err) => {
            // `warn`, not `error`: an unready pod is an expected transient
            // state during rollouts, and the probe will keep polling anyway.
            tracing::warn!(error = %err, "readiness check failed: database unreachable");

            // Non-2xx bodies follow the API-wide error envelope, so a
            // failing probe reads exactly like any other failure.
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "database unreachable" })),
            )
        }
    }
}

/// Answers any request that matched no route.
///
/// Lives next to the routing table on purpose: it is a property of the
/// router, not a resource handler. Returning [`ApiError::NotFound`]
/// instead of axum's default empty 404 keeps unknown paths on the same
/// JSON error envelope as the rest of the API.
async fn not_found() -> ApiError {
    ApiError::NotFound
}
