//! API-side domain models.
//!
//! Keep these decoupled from the database entities of the `entity` crate
//! (reachable as `::entity` from this crate), so the domain view can evolve
//! independently of the schema. The wire format itself (request/response
//! payloads) belongs to [`crate::dto`].
//!
//! Add one module per resource and re-export it here:
//!
//! ```text
//! mod subnet;
//! pub use subnet::*;
//! ```
