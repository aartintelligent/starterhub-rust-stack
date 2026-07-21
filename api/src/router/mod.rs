//! Router assembly.
//!
//! The single place where URLs are mapped to handlers. Group new endpoints
//! by resource and prefer `Router::nest` as the surface grows.
//!
//! Technical endpoints — Kubernetes health probes and the 404 fallback —
//! are defined here as well: they are properties of the routing table
//! itself, while business handlers live in [`crate::handler`].
//!
//! The OpenAPI document ([`ApiDoc`]) also lives here: the routing table
//! and its public contract must evolve together, so keeping them in one
//! file makes a missing annotation visible in code review. Every handler
//! wired below carries a `#[utoipa::path]` annotation and is listed in
//! the `paths(...)` attribute of [`ApiDoc`].

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::{Router, routing::get};
use serde_json::{Value, json};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::error::ApiError;
use crate::state::AppState;

/// The OpenAPI description of this API, derived at compile time.
///
/// The version defaults to the crate metadata, so the document version
/// follows the workspace release automatically. The description stays
/// short and non-technical — the endpoint annotations carry the
/// technical details. Register every new `#[utoipa::path]`-annotated
/// handler in `paths(...)`: an endpoint absent from this list is
/// invisible in Swagger UI.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "rust-service-starter",
        description = "A fast web service, built in Rust.",
        license(name = "MIT", identifier = "MIT")
    ),
    paths(livez, readyz),
    tags(
        (name = "probe", description = "Kubernetes health probes")
    )
)]
struct ApiDoc;

/// Builds the top-level router with `state` attached.
///
/// Health endpoints follow the Kubernetes probe conventions: `/livez` and
/// its legacy alias `/healthz` for liveness, `/readyz` for readiness.
/// Swagger UI is served on `/docs`, backed by the generated document at
/// `/api-docs/openapi.json`.
pub fn router(state: AppState) -> Router {
    Router::new()
        // Interactive documentation. The UI itself is static HTML/JS —
        // the JSON-only rule targets API responses, and the underlying
        // contract (`/api-docs/openapi.json`) is JSON like everything
        // else.
        .merge(SwaggerUi::new("/docs").url("/api-docs/openapi.json", ApiDoc::openapi()))
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
#[utoipa::path(
    get,
    path = "/livez",
    tag = "probe",
    responses(
        (status = OK, description = "Process is alive", body = Value,
         example = json!({ "status": "ok" }))
    )
)]
async fn livez() -> (StatusCode, Json<Value>) {
    (StatusCode::OK, Json(json!({ "status": "ok" })))
}

/// Readiness probe: reports whether the service can handle traffic.
///
/// Pings the database through the shared pool; a failure yields
/// `503 Service Unavailable` so the pod is removed from load balancing
/// without being restarted.
#[utoipa::path(
    get,
    path = "/readyz",
    tag = "probe",
    responses(
        (status = OK, description = "Service is ready for traffic", body = Value,
         example = json!({ "status": "ok" })),
        (status = SERVICE_UNAVAILABLE, description = "A dependency is unreachable", body = Value,
         example = json!({ "error": "database unreachable" }))
    )
)]
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
