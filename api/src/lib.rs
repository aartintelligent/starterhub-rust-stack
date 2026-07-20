//! HTTP API of the application.
//!
//! The crate is organised by responsibility:
//!
//! - [`dto`]: data transfer objects (request payloads, response bodies).
//! - [`entity`]: API-side domain models, decoupled from the database.
//! - [`error`]: unified error type mapped to HTTP responses.
//! - [`extract`]: crate-local extractors rejecting through [`error::ApiError`].
//! - [`handler`]: axum handlers, kept thin and delegating to services.
//! - [`middleware`]: cross-cutting axum middlewares (auth, logging, ...).
//! - [`router`]: router assembly, the single place where URLs are declared.
//! - [`server`]: HTTP server bootstrap.
//! - [`service`]: business logic, split between read ([`service::Query`])
//!   and write ([`service::Mutation`]) operations.
//! - [`state`]: shared state injected into every handler.

pub mod dto;
pub mod entity;
pub mod error;
pub mod extract;
pub mod handler;
pub mod middleware;
pub mod router;
pub mod server;
pub mod service;
pub mod state;
